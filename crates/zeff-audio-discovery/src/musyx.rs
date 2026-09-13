use std::collections::BTreeSet;
use std::mem::size_of;
use std::sync::atomic::AtomicBool;

use anyhow::{Result as AnyResult, ensure};
use serde::Serialize;

use super::{Budget, MAX_ROM_BYTES, RomSpan, ScanStop, word};

mod bootstrap;
mod driver;
mod signatures;
#[cfg(test)]
mod tests;

const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;
const MAX_VALIDATION_WORK: u64 = 100_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MusyxSong {
    pub root: RomSpan,
    pub header: RomSpan,
    pub index: u16,
    pub title: String,
    pub channels: u8,
    pub tempo: u16,
    pub patterns: u16,
    pub notes: u32,
    pub instruments: u16,
    pub samples: u16,
    pub native: MusyxNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MusyxNativeProfile {
    pub init: RomSpan,
    pub select: RomSpan,
    pub start: RomSpan,
    pub update: RomSpan,
    pub timer1_irq: RomSpan,
    pub state_pointer: u32,
    pub compact_init: bool,
    pub boot_entry: u32,
    pub configuration_entry: Option<RomSpan>,
    pub setup_spans: Vec<RomSpan>,
}

pub struct PreparedMusyxRom {
    pub bytes: Vec<u8>,
    pub wait_loop: RomSpan,
}

struct Bank {
    root: usize,
    songs: Vec<usize>,
    group_end: usize,
    instruments: u16,
    samples: u16,
    spans: Vec<RomSpan>,
}

#[derive(Debug)]
enum ReadError {
    Invalid,
    Stop(ScanStop),
}

impl From<ScanStop> for ReadError {
    fn from(value: ScanStop) -> Self {
        Self::Stop(value)
    }
}
type ReadResult<T> = std::result::Result<T, ReadError>;

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<MusyxSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    scan_with_retained_limit(bytes, songs, budget, max_candidates, MAX_RETAINED_BYTES)
}

fn scan_with_retained_limit(
    bytes: &[u8],
    songs: &mut Vec<MusyxSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
    retained_limit: usize,
) -> Result<(), ScanStop> {
    if songs.len() >= max_candidates {
        return Err(ScanStop::CandidateLimit);
    }
    let mut retained = retained_owned_bytes(songs);
    if retained > retained_limit {
        return Err(ScanStop::InventoryLimit);
    }
    let inventory = driver::recognize(bytes, budget)?;
    let profiles = &inventory.profiles;
    if profiles.is_empty() {
        return Ok(());
    }
    let mut unresolved = false;
    for root in (0..bytes.len().saturating_sub(31)).step_by(4) {
        budget.charge()?;
        if !looks_like_root(bytes, root) {
            continue;
        }
        let bank = match parse_bank(bytes, root, budget) {
            Ok(bank) => bank,
            Err(ReadError::Invalid) => continue,
            Err(ReadError::Stop(stop)) => return Err(stop),
        };
        let image = inventory.image_start(root);
        let candidates = profiles
            .iter()
            .filter(|p| inventory.image_start(p.init.effective_offset as usize) == image)
            .collect::<Vec<_>>();
        let [native] = candidates.as_slice() else {
            unresolved = true;
            continue;
        };
        for (index, &group) in bank.songs.iter().enumerate() {
            budget.charge()?;
            let end = bank.songs.get(index + 1).copied().unwrap_or(bank.group_end);
            let song = match parse_song(bytes, &bank, group..end, index as u16, native, budget) {
                Ok(song) => song,
                Err(ReadError::Invalid) => {
                    unresolved = true;
                    continue;
                }
                Err(ReadError::Stop(stop)) => return Err(stop),
            };
            if songs.len() >= max_candidates {
                return Err(ScanStop::CandidateLimit);
            }
            if bootstrap::placement(bytes, &song).is_none() {
                unresolved = true;
                continue;
            }
            push_song(songs, song, &mut retained, retained_limit)?;
        }
    }
    if unresolved {
        Err(ScanStop::ValidationLimit)
    } else {
        Ok(())
    }
}

fn push_song(
    songs: &mut Vec<MusyxSong>,
    song: MusyxSong,
    retained: &mut usize,
    limit: usize,
) -> Result<(), ScanStop> {
    let owned = song_owned_bytes(&song);
    let available = limit
        .checked_sub(*retained)
        .and_then(|bytes| bytes.checked_sub(owned))
        .ok_or(ScanStop::InventoryLimit)?;
    let mut added_capacity = 0;
    if songs.len() == songs.capacity() {
        if available < size_of::<MusyxSong>() {
            return Err(ScanStop::InventoryLimit);
        }
        let previous = songs.capacity();
        songs
            .try_reserve_exact(1)
            .map_err(|_| ScanStop::InventoryLimit)?;
        added_capacity = (songs.capacity() - previous) * size_of::<MusyxSong>();
        if added_capacity > available {
            return Err(ScanStop::InventoryLimit);
        }
    }
    *retained += owned + added_capacity;
    songs.push(song);
    Ok(())
}

fn retained_owned_bytes(songs: &Vec<MusyxSong>) -> usize {
    songs.iter().map(song_owned_bytes).fold(
        songs.capacity().saturating_mul(size_of::<MusyxSong>()),
        usize::saturating_add,
    )
}

fn song_owned_bytes(song: &MusyxSong) -> usize {
    song.title
        .capacity()
        .saturating_add(
            song.mapped_spans
                .capacity()
                .saturating_mul(size_of::<RomSpan>()),
        )
        .saturating_add(
            song.native
                .setup_spans
                .capacity()
                .saturating_mul(size_of::<RomSpan>()),
        )
        .saturating_add(song.warnings.iter().map(|warning| warning.capacity()).fold(
            song.warnings.capacity().saturating_mul(size_of::<String>()),
            usize::saturating_add,
        ))
}

pub fn prepare_rom(
    bytes: &[u8],
    song: &MusyxSong,
    cancel: &AtomicBool,
) -> AnyResult<PreparedMusyxRom> {
    ensure!(
        bytes.len() <= MAX_ROM_BYTES,
        "MusyX source exceeds the ROM size limit"
    );
    let mut budget = Budget {
        cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let profiles = driver::recognize(bytes, &mut budget)
        .map_err(|e| anyhow::anyhow!("MusyX driver validation stopped: {e:?}"))?;
    ensure!(
        profiles.profiles.contains(&song.native),
        "MusyX native driver no longer matches"
    );
    let bank = parse_bank(bytes, song.root.effective_offset as usize, &mut budget)
        .map_err(|e| anyhow::anyhow!("MusyX bank validation failed: {e:?}"))?;
    let index = usize::from(song.index);
    let group = *bank
        .songs
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("MusyX song selector is unavailable"))?;
    let end = bank.songs.get(index + 1).copied().unwrap_or(bank.group_end);
    let current = parse_song(
        bytes,
        &bank,
        group..end,
        song.index,
        &song.native,
        &mut budget,
    )
    .map_err(|e| anyhow::anyhow!("MusyX song validation failed: {e:?}"))?;
    ensure!(
        current == *song,
        "MusyX retained song no longer matches its source"
    );
    bootstrap::build(bytes, song)
}

fn looks_like_root(bytes: &[u8], root: usize) -> bool {
    [0, 24, 28].into_iter().all(|delta| {
        relative(bytes, root, root + delta, 4)
            .is_ok_and(|p| p >= root + 32 && p - root < 0x1000000 && p.is_multiple_of(4))
    }) && [4, 8, 12].into_iter().all(|delta| {
        word(bytes, root + delta).is_some_and(|value| {
            value == 0
                || relative(bytes, root, root + delta, 4)
                    .is_ok_and(|p| p >= root + 32 && p - root < 0x1000000 && p.is_multiple_of(4))
        })
    })
}

fn parse_bank(bytes: &[u8], root: usize, budget: &mut Budget<'_>) -> ReadResult<Bank> {
    if !looks_like_root(bytes, root) {
        return Err(ReadError::Invalid);
    }
    let instruments = relative(bytes, root, root, 4)?;
    let song_table = relative(bytes, root, root + 24, 4)?;
    let samples = relative(bytes, root, root + 28, 4)?;
    let instrument_first = relative(bytes, root, instruments, 4)?;
    let sample_first = relative(bytes, root, samples, 16)?;
    let instrument_count = table_count(instruments, instrument_first)?;
    let sample_count = table_count(samples, sample_first)?;
    let song_count = read_word(bytes, song_table)? as usize;
    if !(1..=4096).contains(&song_count) {
        return Err(ReadError::Invalid);
    }
    let mut spans = vec![
        span(bytes, root, 32)?,
        span(bytes, instruments, instrument_count * 4)?,
        span(bytes, samples, sample_count * 4)?,
        span(bytes, song_table, 4 + song_count * 4)?,
    ];
    for index in 0..instrument_count {
        budget.charge()?;
        let offset = relative(bytes, root, instruments + index * 4, 4)?;
        if bytes[offset + 2] > 1 {
            return Err(ReadError::Invalid);
        }
        spans.push(span(bytes, offset, 4)?);
    }
    for index in 0..sample_count {
        budget.charge()?;
        let offset = relative(bytes, root, samples + index * 4, 16)?;
        let length = read_word(bytes, offset)? as usize;
        let loop_start = read_word(bytes, offset + 4)? as i32;
        let sample_kind = bytes[offset + 11];
        if length == 0
            || length >= 0x1000000
            || i64::from(loop_start) > length as i64
            || half(bytes, offset + 8)? == 0
            || bytes[offset + 10] > 127
            || sample_kind > 1
            || (sample_kind == 1 && !matches!(length, 16 | 32))
        {
            return Err(ReadError::Invalid);
        }
        spans.push(span(bytes, offset, 16 + length)?);
    }
    let mut songs = Vec::with_capacity(song_count);
    for index in 0..song_count {
        budget.charge()?;
        let group = relative(bytes, root, song_table + 4 + index * 4, 1052)?;
        if !group.is_multiple_of(4)
            || group >= samples
            || songs.last().is_some_and(|previous| *previous >= group)
        {
            return Err(ReadError::Invalid);
        }
        songs.push(group);
    }
    Ok(Bank {
        root,
        songs,
        group_end: samples,
        instruments: instrument_count as u16,
        samples: sample_count as u16,
        spans,
    })
}

fn parse_song(
    bytes: &[u8],
    bank: &Bank,
    group_range: std::ops::Range<usize>,
    index: u16,
    native: &MusyxNativeProfile,
    budget: &mut Budget<'_>,
) -> ReadResult<MusyxSong> {
    let (group, end) = (group_range.start, group_range.end);
    let anchor = group.checked_add(1040).ok_or(ReadError::Invalid)?;
    if anchor + 12 > end {
        return Err(ReadError::Invalid);
    }
    let tracks = relative(bytes, anchor, anchor, 68)?;
    let patterns = relative(bytes, anchor, anchor + 4, 4)?;
    within(tracks, 68, anchor, end)?;
    within(patterns, 4, anchor, end)?;
    let tempo = half(bytes, anchor + 8)?;
    if !(1..=1024).contains(&tempo) {
        return Err(ReadError::Invalid);
    }
    let mut spans = bank.spans.clone();
    spans.extend([span(bytes, group, 1052)?, span(bytes, tracks, 68)?]);
    let mut used = BTreeSet::new();
    let mut channels = 0;
    for channel in 0..17 {
        budget.charge()?;
        if read_word(bytes, tracks + channel * 4)? == 0 {
            continue;
        }
        if channel < 16 {
            channels += 1;
        }
        let start = relative(bytes, anchor, tracks + channel * 4, 16)?;
        let mut offset = start;
        let mut entries = 0;
        loop {
            budget.charge()?;
            within(offset, 8, anchor, end)?;
            let pattern = read_word(bytes, offset + 4)? as i32;
            offset += 8;
            entries += 1;
            if !(-2..4096).contains(&pattern) {
                return Err(ReadError::Invalid);
            }
            if pattern < 0 {
                break;
            }
            used.insert((pattern as usize, channel == 16));
            if entries >= 8192 {
                return Err(ReadError::Stop(ScanStop::ValidationLimit));
            }
        }
        within(offset, 8, anchor, end)?;
        let loop_entry = read_word(bytes, offset)? as i32;
        if loop_entry < 0 || loop_entry % 8 != 0 || loop_entry as usize / 8 >= entries {
            return Err(ReadError::Invalid);
        }
        spans.push(span(bytes, start, offset + 8 - start)?);
    }
    let mut notes = 0u32;
    for &(pattern, control) in &used {
        budget.charge()?;
        let entry = patterns
            .checked_add(pattern * 4)
            .ok_or(ReadError::Invalid)?;
        within(entry, 4, anchor, end)?;
        spans.push(span(bytes, entry, 4)?);
        let start = relative(bytes, anchor, entry, 4)?;
        let mut offset = start;
        let mut events = 0;
        loop {
            budget.charge()?;
            within(offset, 4, anchor, end)?;
            if read_word(bytes, offset)? == u32::MAX {
                offset += 4;
                break;
            }
            let velocity = bytes[offset + 2];
            let note = bytes[offset + 3];
            let length = if control || velocity == 0 {
                4
            } else if note & 128 != 0 {
                8
            } else {
                6
            };
            if !control && velocity > 127 {
                return Err(ReadError::Invalid);
            }
            within(offset, length, anchor, end)?;
            if !control && velocity != 0 {
                notes += 1;
            }
            offset += length;
            events += 1;
            if events >= 65536 {
                return Err(ReadError::Stop(ScanStop::ValidationLimit));
            }
        }
        spans.push(span(bytes, start, offset - start)?);
    }
    spans.extend(native.setup_spans.iter().copied());
    spans.sort_unstable();
    spans.dedup();
    Ok(MusyxSong {
        root: RomSpan::new(bank.root, 32), header: RomSpan::new(group, 1052), index,
        title: format!("Song {index}"), channels, tempo, patterns: used.len() as u16, notes,
        instruments: bank.instruments, samples: bank.samples, native: native.clone(), mapped_spans: spans,
        warnings: vec!["Runs the original MusyX driver with the source's initialization settings in an isolated GBA emulator.".to_owned(),
            "Song groups may include sound effects or silence. Macro execution is native; the retained asset graph does not describe every macro or keymap relationship.".to_owned()],
    })
}

fn table_count(start: usize, first: usize) -> ReadResult<usize> {
    let length = first.checked_sub(start).ok_or(ReadError::Invalid)?;
    if length == 0 || !length.is_multiple_of(4) || length / 4 > 4096 {
        return Err(ReadError::Invalid);
    }
    Ok(length / 4)
}
fn relative(bytes: &[u8], anchor: usize, at: usize, length: usize) -> ReadResult<usize> {
    let offset = read_word(bytes, at)? as usize;
    if offset == 0 {
        return Err(ReadError::Invalid);
    }
    let target = anchor.checked_add(offset).ok_or(ReadError::Invalid)?;
    span(bytes, target, length)?;
    Ok(target)
}
fn read_word(bytes: &[u8], at: usize) -> ReadResult<u32> {
    word(bytes, at).ok_or(ReadError::Invalid)
}
fn half(bytes: &[u8], at: usize) -> ReadResult<u16> {
    let data = bytes
        .get(at..at.checked_add(2).ok_or(ReadError::Invalid)?)
        .ok_or(ReadError::Invalid)?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}
fn span(bytes: &[u8], offset: usize, length: usize) -> ReadResult<RomSpan> {
    if length == 0
        || offset
            .checked_add(length)
            .is_none_or(|end| end > bytes.len() || end > MAX_ROM_BYTES)
    {
        return Err(ReadError::Invalid);
    }
    Ok(RomSpan::new(offset, length))
}
fn within(offset: usize, length: usize, start: usize, end: usize) -> ReadResult<()> {
    if offset < start || offset.checked_add(length).is_none_or(|next| next > end) {
        return Err(ReadError::Invalid);
    }
    Ok(())
}

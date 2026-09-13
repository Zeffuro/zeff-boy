use std::sync::atomic::AtomicBool;

use serde::Serialize;

use super::{Budget, RomSpan, ScanStop};

mod banks;
mod bootstrap;
mod driver;
mod module;
mod signatures;
mod startup;
#[cfg(test)]
mod tests;

const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;
const MAX_VALIDATION_WORK: u64 = 64_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternEncoding {
    Packed2003,
    Extended2004,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct KrawallSong {
    pub profile: &'static str,
    pub index: u16,
    pub subsong: u8,
    pub title: String,
    pub header: RomSpan,
    pub channels: u8,
    pub start_order: u8,
    pub order_count: u8,
    pub pattern_count: u16,
    pub note_count: u32,
    pub instrument_count: u16,
    pub sample_count: u16,
    pub initial_speed: u8,
    pub initial_bpm: u8,
    pub native: KrawallNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct KrawallNativeProfile {
    pub encoding: PatternEncoding,
    pub process_row: RomSpan,
    pub instrument_bank: u32,
    pub sample_bank: u32,
    pub init: NativeEntry,
    pub play: NativeEntry,
    pub instrument_update: NativeEntry,
    pub mixer: NativeEntry,
    pub timer1_irq: NativeEntry,
    pub ram_copies: Vec<RamCopy>,
    pub setup_spans: Vec<RomSpan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct NativeEntry {
    pub source: RomSpan,
    pub cpu_address: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RamCopy {
    pub source: RomSpan,
    pub destination: u32,
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<KrawallSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    let Some(native) = driver::recognize(bytes, budget)? else {
        return Ok(());
    };
    let mut retained = 0usize;
    let mut unresolved = false;
    for offset in (0..bytes.len().saturating_sub(367)).step_by(4) {
        budget.charge()?;
        if !module::looks_like_header(bytes, offset) {
            continue;
        }
        let parsed = match module::inspect(bytes, offset, &native, budget) {
            Ok(parsed) => parsed,
            Err(ReadError::Invalid) => continue,
            Err(ReadError::UnsupportedGraph) => {
                unresolved = true;
                continue;
            }
            Err(ReadError::Stop(stop)) => return Err(stop),
        };
        for (subsong, start) in parsed.subsongs.iter().copied() {
            budget.charge()?;
            if songs.len() >= max_candidates || songs.len() > u16::MAX as usize {
                return Err(ScanStop::CandidateLimit);
            }
            let song = parsed.song(songs.len() as u16, subsong, start, &native);
            if bootstrap::placement(bytes, &song).is_none() {
                continue;
            }
            retained += std::mem::size_of::<KrawallSong>()
                + song.mapped_spans.len() * std::mem::size_of::<RomSpan>()
                + song.native.setup_spans.len() * std::mem::size_of::<RomSpan>()
                + song.native.ram_copies.len() * std::mem::size_of::<RamCopy>()
                + song.title.len()
                + song.warnings.iter().map(String::len).sum::<usize>();
            if retained > MAX_RETAINED_BYTES {
                return Err(ScanStop::ValidationLimit);
            }
            songs.push(song);
        }
    }
    if unresolved {
        Err(ScanStop::ValidationLimit)
    } else {
        Ok(())
    }
}

pub fn validate_song(bytes: &[u8], song: &KrawallSong, cancel: &AtomicBool) -> anyhow::Result<()> {
    checked_song(bytes, song, cancel).map(|_| ())
}

pub fn prepare_rom(
    bytes: &[u8],
    song: &KrawallSong,
    cancel: &AtomicBool,
) -> anyhow::Result<Vec<u8>> {
    checked_song(bytes, song, cancel)?;
    bootstrap::build(bytes, song)
}

fn checked_song(
    bytes: &[u8],
    song: &KrawallSong,
    cancel: &AtomicBool,
) -> anyhow::Result<KrawallSong> {
    anyhow::ensure!(
        bytes.len() <= super::MAX_ROM_BYTES,
        "Krawall ROM exceeds the size limit"
    );
    let mut budget = Budget {
        cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let native = driver::recognize(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Krawall validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Krawall driver or startup profile no longer matches"))?;
    let parsed = module::inspect(
        bytes,
        song.header.effective_offset as usize,
        &native,
        &mut budget,
    )
    .map_err(|error| anyhow::anyhow!("Krawall module validation failed: {error:?}"))?;
    let start = parsed
        .subsongs
        .iter()
        .find_map(|&(selector, start)| (selector == song.subsong).then_some(start))
        .ok_or_else(|| anyhow::anyhow!("Krawall subsong selector no longer matches"))?;
    let current = parsed.song(song.index, song.subsong, start, &native);
    anyhow::ensure!(
        current == *song,
        "Krawall selected song or native setup changed"
    );
    Ok(current)
}

#[derive(Debug)]
enum ReadError {
    Invalid,
    UnsupportedGraph,
    Stop(ScanStop),
}

impl From<ScanStop> for ReadError {
    fn from(value: ScanStop) -> Self {
        Self::Stop(value)
    }
}

fn range(bytes: &[u8], offset: usize, len: usize) -> Result<&[u8], ReadError> {
    bytes
        .get(offset..offset.checked_add(len).ok_or(ReadError::Invalid)?)
        .ok_or(ReadError::Invalid)
}

fn half(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn pointer(bytes: &[u8], slot: usize, len: usize, align: usize) -> Result<usize, ReadError> {
    let address = super::word(bytes, slot).ok_or(ReadError::Invalid)?;
    super::rom_pointer(bytes, address, len, align).ok_or(ReadError::Invalid)
}

fn merge_spans(spans: &mut Vec<RomSpan>) {
    spans.sort_unstable_by_key(|span| (span.effective_offset, span.byte_len));
    let mut write = 0usize;
    for read in 0..spans.len() {
        let span = spans[read];
        if write > 0 {
            let previous = &mut spans[write - 1];
            let end = previous.effective_offset + previous.byte_len;
            if span.effective_offset <= end {
                previous.byte_len =
                    end.max(span.effective_offset + span.byte_len) - previous.effective_offset;
                continue;
            }
        }
        spans[write] = span;
        write += 1;
    }
    spans.truncate(write);
}

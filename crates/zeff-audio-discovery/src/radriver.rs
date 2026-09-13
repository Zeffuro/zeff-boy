use std::sync::atomic::AtomicBool;

use anyhow::{Result as AnyResult, ensure};
use serde::Serialize;

use super::{Budget, MAX_ROM_BYTES, RomSpan, ScanStop, word};

mod bootstrap;
mod codec;
mod data;
mod driver;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
mod signatures;
mod startup;
#[cfg(test)]
mod tests;

const MAX_TABLE_ENTRIES: usize = 1024;
const MAX_ORDERS: usize = 16_384;
const MAX_VALIDATION_WORK: u64 = 100_000_000;
const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RadriverSongKind {
    Effect,
    CompressedMusic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RadriverLayout {
    GlobalState,
    ContextState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RadriverSample {
    pub header: RomSpan,
    pub data: RomSpan,
    pub padding: Option<RomSpan>,
    pub encoding: u8,
    pub loop_start: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RadriverSong {
    pub root: RomSpan,
    pub header: RomSpan,
    pub index: u16,
    pub kind: RadriverSongKind,
    pub title: String,
    pub order: Option<RomSpan>,
    pub blocks: Option<RomSpan>,
    pub samples: Vec<RadriverSample>,
    pub native: RadriverNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RadriverNativeProfile {
    pub layout: RadriverLayout,
    pub init: RomSpan,
    pub handoff: RomSpan,
    pub play_effect: RomSpan,
    pub play_music: Option<RomSpan>,
    pub update: RomSpan,
    pub timer_irq: RomSpan,
    pub bank: RomSpan,
    pub music_table: Option<RomSpan>,
    pub state: u32,
    pub sample_rate: u32,
    pub channels: u8,
    pub setup_spans: Vec<RomSpan>,
}

pub struct PreparedRadriverRom {
    pub bytes: Vec<u8>,
    pub wait_loop: RomSpan,
}

#[derive(Debug, PartialEq, Eq)]
enum ReadError {
    Invalid,
    UnboundStartup,
    Stop(ScanStop),
}

impl From<ScanStop> for ReadError {
    fn from(value: ScanStop) -> Self {
        Self::Stop(value)
    }
}

type ReadResult<T> = Result<T, ReadError>;

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<RadriverSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    if songs.len() >= max_candidates {
        return Err(ScanStop::CandidateLimit);
    }
    let mut retained = songs.capacity().saturating_mul(size_of::<RadriverSong>());
    retained = songs
        .iter()
        .map(owned_bytes)
        .fold(retained, usize::saturating_add);
    if retained > MAX_RETAINED_BYTES {
        return Err(ScanStop::InventoryLimit);
    }
    let (profiles, mut unresolved) = driver::recognize(bytes, budget)?;
    for native in profiles {
        if native.layout == RadriverLayout::GlobalState
            && word(bytes, native.bank.effective_offset as usize).unwrap_or(0) != 0
        {
            unresolved = true;
        }
        let effects = word(bytes, native.bank.effective_offset as usize + 4).unwrap_or(0);
        let music = native.music_table.map_or(0, |table| table.byte_len / 8);
        for (kind, count) in [
            (RadriverSongKind::Effect, effects),
            (RadriverSongKind::CompressedMusic, music),
        ] {
            for index in 0..count {
                let song = match data::parse_song(bytes, &native, kind, index as u16, budget) {
                    Ok(song) => song,
                    Err(ReadError::Invalid | ReadError::UnboundStartup) => {
                        unresolved = true;
                        continue;
                    }
                    Err(ReadError::Stop(stop)) => return Err(stop),
                };
                if bootstrap::placement(bytes).is_none() {
                    unresolved = true;
                    continue;
                }
                if songs.len() >= max_candidates {
                    return Err(ScanStop::CandidateLimit);
                }
                let available = MAX_RETAINED_BYTES
                    .checked_sub(retained)
                    .and_then(|n| n.checked_sub(owned_bytes(&song)))
                    .ok_or(ScanStop::InventoryLimit)?;
                let old_capacity = songs.capacity();
                if songs.len() == old_capacity {
                    if available < size_of::<RadriverSong>() {
                        return Err(ScanStop::InventoryLimit);
                    }
                    songs
                        .try_reserve_exact(1)
                        .map_err(|_| ScanStop::InventoryLimit)?;
                }
                let added = (songs.capacity() - old_capacity) * size_of::<RadriverSong>();
                if added > available {
                    return Err(ScanStop::InventoryLimit);
                }
                retained += owned_bytes(&song) + added;
                songs.push(song);
            }
        }
    }
    if unresolved {
        Err(ScanStop::ValidationLimit)
    } else {
        Ok(())
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn fixture_rom() -> Vec<u8> {
    fixture::build()
}

#[cfg(any(test, feature = "test-support"))]
pub fn fixture_global_rom() -> Vec<u8> {
    fixture::global()
}

pub fn prepare_rom(
    bytes: &[u8],
    song: &RadriverSong,
    cancel: &AtomicBool,
) -> AnyResult<PreparedRadriverRom> {
    ensure!(
        bytes.len() <= MAX_ROM_BYTES,
        "RADriver source exceeds the ROM size limit"
    );
    let mut budget = Budget {
        cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let (profiles, _) = driver::recognize(bytes, &mut budget)
        .map_err(|e| anyhow::anyhow!("RADriver driver validation stopped: {e:?}"))?;
    ensure!(
        profiles.contains(&song.native),
        "RADriver native driver no longer matches"
    );
    let current = data::parse_song(bytes, &song.native, song.kind, song.index, &mut budget)
        .map_err(|e| anyhow::anyhow!("RADriver selection validation failed: {e:?}"))?;
    ensure!(
        current == *song,
        "RADriver retained selection no longer matches its source"
    );
    bootstrap::build(bytes, song)
}

fn owned_bytes(song: &RadriverSong) -> usize {
    song.title
        .capacity()
        .saturating_add(
            song.samples
                .capacity()
                .saturating_mul(size_of::<RadriverSample>()),
        )
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
        .saturating_add(song.warnings.iter().map(String::capacity).fold(
            song.warnings.capacity().saturating_mul(size_of::<String>()),
            usize::saturating_add,
        ))
}

fn half(bytes: &[u8], at: usize) -> Option<u16> {
    let value = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([value[0], value[1]]))
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

fn pointer(bytes: &[u8], address: u32, length: usize) -> ReadResult<RomSpan> {
    let offset = address.checked_sub(0x0800_0000).ok_or(ReadError::Invalid)? as usize;
    if !offset.is_multiple_of(4) {
        return Err(ReadError::Invalid);
    }
    span(bytes, offset, length)
}

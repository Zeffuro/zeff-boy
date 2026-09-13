use std::sync::atomic::AtomicBool;

use anyhow::{Result as AnyResult, ensure};
use serde::Serialize;

use super::{Budget, MAX_ROM_BYTES, RomSpan, ScanStop, word};

mod bootstrap;
mod data;
mod driver;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
mod signatures;
#[cfg(test)]
mod tests;
mod wod;

#[cfg(any(test, feature = "test-support"))]
pub fn fixture_rom() -> Vec<u8> {
    fixture::build(0)
}

const MAX_EVENTS: usize = 16_384;
const MAX_TABLE_ENTRIES: usize = 1024;
const MAX_VALIDATION_WORK: u64 = 100_000_000;
const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NsqSong {
    pub root: RomSpan,
    pub header: RomSpan,
    pub sequence: RomSpan,
    pub bank: RomSpan,
    pub index: u16,
    pub slot: u16,
    pub title: String,
    pub notes: u32,
    pub duration_frames: u32,
    pub instruments: u16,
    pub samples: u16,
    pub native: NsqNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NsqNativeProfile {
    pub load_bank: RomSpan,
    pub load_songs: RomSpan,
    pub play: RomSpan,
    pub vblank: RomSpan,
    pub mix: RomSpan,
    pub song_table: RomSpan,
    pub filesystem: RomSpan,
    pub bank_directory: RomSpan,
    pub instrument_path: RomSpan,
    pub release_prefix: RomSpan,
    pub setup_spans: Vec<RomSpan>,
    pub bank_arguments: Vec<[u32; 2]>,
}

pub struct PreparedNsqRom {
    pub bytes: Vec<u8>,
    pub wait_loop: RomSpan,
}

#[derive(Debug, PartialEq, Eq)]
enum ReadError {
    Invalid,
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
    songs: &mut Vec<NsqSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    let profiles = driver::recognize(bytes, budget)?;
    let mut retained = songs.capacity().saturating_mul(size_of::<NsqSong>());
    retained = songs
        .iter()
        .map(owned_bytes)
        .fold(retained, usize::saturating_add);
    let mut unresolved = false;
    for native in profiles {
        let count = native.song_table.byte_len as usize / 8 - 1;
        for slot in 0..count {
            let song = match data::parse_song(bytes, &native, slot, budget) {
                Ok(song) => song,
                Err(ReadError::Invalid) => {
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
                if available < size_of::<NsqSong>() {
                    return Err(ScanStop::InventoryLimit);
                }
                songs
                    .try_reserve_exact(1)
                    .map_err(|_| ScanStop::InventoryLimit)?;
            }
            let added = (songs.capacity() - old_capacity) * size_of::<NsqSong>();
            if added > available {
                return Err(ScanStop::InventoryLimit);
            }
            retained += owned_bytes(&song) + added;
            songs.push(song);
        }
    }
    if unresolved {
        Err(ScanStop::ValidationLimit)
    } else {
        Ok(())
    }
}

pub fn prepare_rom(bytes: &[u8], song: &NsqSong, cancel: &AtomicBool) -> AnyResult<PreparedNsqRom> {
    ensure!(
        bytes.len() <= MAX_ROM_BYTES,
        "NSQ source exceeds the ROM size limit"
    );
    let mut budget = Budget {
        cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let profiles = driver::recognize(bytes, &mut budget)
        .map_err(|e| anyhow::anyhow!("NSQ driver validation stopped: {e:?}"))?;
    ensure!(
        profiles.contains(&song.native),
        "NSQ native driver no longer matches"
    );
    let current = data::parse_song(bytes, &song.native, usize::from(song.slot), &mut budget)
        .map_err(|e| anyhow::anyhow!("NSQ song validation failed: {e:?}"))?;
    ensure!(
        current == *song,
        "NSQ retained song no longer matches its source"
    );
    bootstrap::build(bytes, song)
}

fn owned_bytes(song: &NsqSong) -> usize {
    song.title
        .capacity()
        .saturating_add(
            song.mapped_spans
                .capacity()
                .saturating_mul(size_of::<RomSpan>()),
        )
        .saturating_add(
            song.native
                .bank_arguments
                .capacity()
                .saturating_mul(size_of::<[u32; 2]>()),
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

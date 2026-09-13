use std::mem::size_of;
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

#[cfg(any(test, feature = "test-support"))]
pub fn fixture_rom(current: bool) -> Vec<u8> {
    fixture::fixture(current)
}
#[cfg(test)]
mod tests;

const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;
const MAX_VALIDATION_WORK: u64 = 100_000_000;
const MAX_ORDERS: usize = 128;
const INSTRUMENTS_PER_SONG: usize = 31;
const SAMPLE_HEADER_BYTES: usize = 12;
const PATTERN_ROWS: usize = 64;
const PATTERN_BYTES: usize = PATTERN_ROWS * size_of::<u32>();

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AasSong {
    pub root: RomSpan,
    pub header: RomSpan,
    pub index: u16,
    pub title: String,
    pub channels: u8,
    pub orders: u16,
    pub patterns: u16,
    pub notes: u32,
    pub instruments: u16,
    pub samples: u16,
    pub native: AasNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AasNativeProfile {
    pub config: RomSpan,
    pub play: RomSpan,
    pub stop: RomSpan,
    pub update: RomSpan,
    pub timer1_irq: RomSpan,
    pub max_channels: u8,
    pub config_state: u32,
    pub song_state: u32,
    pub tables: AasTables,
    pub setup_spans: Vec<RomSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AasTables {
    pub count: RomSpan,
    pub sample_headers: RomSpan,
    pub sequence: RomSpan,
    pub channels: RomSpan,
    pub restart: RomSpan,
    pub sample_data: RomSpan,
    pub pattern_data: RomSpan,
}

pub struct PreparedAasRom {
    pub bytes: Vec<u8>,
    pub wait_loop: RomSpan,
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
type ReadResult<T> = Result<T, ReadError>;

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<AasSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    scan_with_retained_limit(bytes, songs, budget, max_candidates, MAX_RETAINED_BYTES)
}

fn scan_with_retained_limit(
    bytes: &[u8],
    songs: &mut Vec<AasSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
    retained_limit: usize,
) -> Result<(), ScanStop> {
    let mut retained = retained_owned_bytes(songs);
    if retained > retained_limit {
        return Err(ScanStop::InventoryLimit);
    }
    let profiles = driver::recognize(bytes, budget)?;
    let mut unresolved = false;
    for native in profiles {
        let count = half(bytes, native.tables.count.effective_offset as usize)
            .expect("driver validated the count");
        for index in 0..count {
            let song = match data::parse_song(bytes, &native, index, budget) {
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
            push_song(songs, song, &mut retained, retained_limit)?;
        }
    }
    if unresolved {
        Err(ScanStop::ValidationLimit)
    } else {
        Ok(())
    }
}

pub fn prepare_rom(bytes: &[u8], song: &AasSong, cancel: &AtomicBool) -> AnyResult<PreparedAasRom> {
    ensure!(
        bytes.len() <= MAX_ROM_BYTES,
        "AAS source exceeds the ROM size limit"
    );
    let mut budget = Budget {
        cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let profiles = driver::recognize(bytes, &mut budget)
        .map_err(|e| anyhow::anyhow!("AAS driver validation stopped: {e:?}"))?;
    ensure!(
        profiles.contains(&song.native),
        "AAS native driver no longer matches"
    );
    let current = data::parse_song(bytes, &song.native, song.index, &mut budget)
        .map_err(|e| anyhow::anyhow!("AAS song validation failed: {e:?}"))?;
    ensure!(
        current == *song,
        "AAS retained song no longer matches its source"
    );
    bootstrap::build(bytes, song)
}

fn push_song(
    songs: &mut Vec<AasSong>,
    song: AasSong,
    retained: &mut usize,
    limit: usize,
) -> Result<(), ScanStop> {
    let owned = song_owned_bytes(&song);
    let available = limit
        .checked_sub(*retained)
        .and_then(|n| n.checked_sub(owned))
        .ok_or(ScanStop::InventoryLimit)?;
    let mut added = 0;
    if songs.len() == songs.capacity() {
        if available < size_of::<AasSong>() {
            return Err(ScanStop::InventoryLimit);
        }
        let previous = songs.capacity();
        songs
            .try_reserve_exact(1)
            .map_err(|_| ScanStop::InventoryLimit)?;
        added = (songs.capacity() - previous) * size_of::<AasSong>();
        if added > available {
            return Err(ScanStop::InventoryLimit);
        }
    }
    *retained += owned + added;
    songs.push(song);
    Ok(())
}

fn retained_owned_bytes(songs: &Vec<AasSong>) -> usize {
    songs.iter().map(song_owned_bytes).fold(
        songs.capacity().saturating_mul(size_of::<AasSong>()),
        usize::saturating_add,
    )
}

fn song_owned_bytes(song: &AasSong) -> usize {
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

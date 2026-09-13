use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop};

mod native;
mod profiles;
mod sequence;
#[cfg(any(test, feature = "test-support"))]
mod tests;

pub use native::prepare_rom;
#[cfg(feature = "test-support")]
pub use tests::synthetic_rom;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GbGhxHardware {
    CgbDouble,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbGhxSong {
    pub profile: &'static str,
    pub index: u16,
    pub module: u8,
    pub subsong: u8,
    pub title: String,
    pub bank: u16,
    pub hardware: GbGhxHardware,
    pub table_entry: RomSpan,
    pub tracks: Vec<GbGhxTrack>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbGhxTrack {
    pub number: u8,
    pub note_count: u32,
}

pub struct PreparedGbGhx {
    pub bytes: Vec<u8>,
    pub hardware: GbGhxHardware,
    pub ready_address: u16,
    pub ready_value: u8,
    pub ack_address: u16,
    pub ack_value: u8,
    pub wait_start: u16,
    pub wait_end: u16,
}

pub fn supports_cartridge(bytes: &[u8]) -> bool {
    bytes.len() >= 0x8000
        && bytes[0x143] == 0xc0
        && (0x19..=0x1e).contains(&bytes[0x147])
        && bytes[0x148] <= 8
        && bytes.len() == (0x8000usize << bytes[0x148])
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

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbGhxSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    for driver in profiles::recognized(bytes, budget)? {
        for module in 0..=255u8 {
            budget.charge()?;
            let table = usize::from(driver.table) + usize::from(module) * 2;
            if table + 2 > 0x8000 {
                break;
            }
            let header = usize::from(word(bytes, driver.offset(table)));
            if !(0x4000..=0x7ff4).contains(&header)
                || &bytes[driver.offset(header)..driver.offset(header) + 3] != b"GHX"
            {
                break;
            }
            for subsong in 0..bytes[driver.offset(header) + 3] {
                match sequence::song(bytes, &driver, module, subsong, budget) {
                    Ok(song) => {
                        if songs.len() >= remaining {
                            return Err(ScanStop::CandidateLimit);
                        }
                        songs.push(song);
                    }
                    Err(ReadError::Invalid) => (),
                    Err(ReadError::Stop(stop)) => return Err(stop),
                }
            }
        }
    }
    Ok(())
}

fn checked_driver(
    bytes: &[u8],
    expected: &GbGhxSong,
    cancel: &AtomicBool,
) -> anyhow::Result<profiles::Driver> {
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let drivers = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Game Boy GHX validation stopped: {stop:?}"))?;
    for driver in drivers {
        if driver.profile.name == expected.profile
            && driver.bank == expected.bank
            && let Ok(checked) = sequence::song(
                bytes,
                &driver,
                expected.module,
                expected.subsong,
                &mut budget,
            )
            && checked == *expected
        {
            return Ok(driver);
        }
    }
    anyhow::bail!("Game Boy GHX inventory differs from its recognized source")
}

pub fn validate_song(bytes: &[u8], song: &GbGhxSong, cancel: &AtomicBool) -> anyhow::Result<()> {
    checked_driver(bytes, song, cancel).map(|_| ())
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn span(offset: usize, len: usize) -> RomSpan {
    RomSpan {
        effective_offset: offset as u32,
        byte_len: len as u32,
        canonical_cpu_address: (0x4000 + offset % 0x4000) as u32,
    }
}

fn spans(mut ranges: Vec<(usize, usize)>) -> Vec<RomSpan> {
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start / 0x4000 == last.0 / 0x4000
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged.into_iter().map(|(a, b)| span(a, b - a)).collect()
}

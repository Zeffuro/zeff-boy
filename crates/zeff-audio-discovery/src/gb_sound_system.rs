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
pub use tests::{synthetic_rom, synthetic_rom_with_hardware};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GbSoundSystemHardware {
    CgbNormal,
    CgbDouble,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbSoundSystemSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub bank: u16,
    pub order_address: u16,
    pub instrument_address: u16,
    pub hardware: GbSoundSystemHardware,
    pub table_entry: RomSpan,
    pub tracks: Vec<GbSoundSystemTrack>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbSoundSystemTrack {
    pub number: u8,
    pub note_count: u32,
}

pub struct PreparedGbSoundSystem {
    pub bytes: Vec<u8>,
    pub hardware: GbSoundSystemHardware,
    pub ready_address: u16,
    pub ready_value: u8,
    pub ack_address: u16,
    pub ack_value: u8,
    pub wait_start: u16,
    pub wait_end: u16,
}

pub fn supports_cartridge(bytes: &[u8]) -> bool {
    bytes.len() >= 0x8000
        && matches!(bytes[0x143], 0x80 | 0xc0)
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
    songs: &mut Vec<GbSoundSystemSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    for driver in profiles::recognized(bytes, budget)? {
        for index in 0..driver.count {
            match sequence::song(bytes, &driver, index, budget) {
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
    Ok(())
}

fn checked_driver(
    bytes: &[u8],
    expected: &GbSoundSystemSong,
    cancel: &AtomicBool,
) -> anyhow::Result<profiles::Driver> {
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let drivers = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("GB Sound System validation stopped: {stop:?}"))?;
    for driver in drivers {
        if driver.profile.name == expected.profile
            && driver.bank == expected.bank
            && let Ok(checked) = sequence::song(bytes, &driver, expected.index, &mut budget)
            && checked == *expected
        {
            return Ok(driver);
        }
    }
    anyhow::bail!("GB Sound System inventory differs from its recognized source")
}

pub fn validate_song(
    bytes: &[u8],
    song: &GbSoundSystemSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    checked_driver(bytes, song, cancel).map(|_| ())
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn span(bank: u16, address: u16, len: usize) -> RomSpan {
    RomSpan {
        effective_offset: u32::from(bank) * 0x4000 + u32::from(address) - 0x4000,
        byte_len: len as u32,
        canonical_cpu_address: u32::from(address),
    }
}

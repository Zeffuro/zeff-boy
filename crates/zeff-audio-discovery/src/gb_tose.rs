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
pub enum GbToseHardware {
    Dmg,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbToseSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub bank: u8,
    pub hardware: GbToseHardware,
    pub table_entry: RomSpan,
    pub tracks: Vec<GbToseTrack>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbToseTrack {
    pub number: u8,
    pub note_count: u32,
}

pub struct PreparedGbTose {
    pub bytes: Vec<u8>,
    pub ready_address: u16,
    pub ready_value: u8,
    pub ack_address: u16,
    pub ack_value: u8,
    pub wait_start: u16,
    pub wait_end: u16,
}

pub fn supports_cartridge(bytes: &[u8]) -> bool {
    bytes.len() >= 0x8000
        && !matches!(bytes[0x143], 0x80 | 0xc0)
        && (1..=3).contains(&bytes[0x147])
        && bytes[0x148] <= 6
        && bytes.len() == (0x8000usize << bytes[0x148])
        && !(bytes.len() == 0x100000
            && [16, 32, 48].into_iter().all(|bank| {
                bytes[bank * 0x4000 + 0x104..bank * 0x4000 + 0x134] == bytes[0x104..0x134]
            }))
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
    songs: &mut Vec<GbToseSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    for driver in profiles::recognized(bytes, budget)? {
        for bank in 1..bytes.len() / 0x4000 {
            if bank % 32 == 0 {
                continue;
            }
            for index in 0..=252 {
                budget.charge()?;
                match song(bytes, driver, bank as u8, index, budget) {
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

fn song(
    bytes: &[u8],
    driver: profiles::Driver,
    bank: u8,
    index: u16,
    budget: &mut Budget<'_>,
) -> Result<GbToseSong, ReadError> {
    let table = usize::from(driver.table) + usize::from(index) * 4;
    if table + 16 > 0x8000 {
        return Err(ReadError::Invalid);
    }
    let table_offset = usize::from(bank) * 0x4000 + table - 0x4000;
    let entries = bytes
        .get(table_offset..table_offset + 16)
        .ok_or(ReadError::Invalid)?;
    if !(0..4).all(|channel| {
        entries[channel * 4] == (channel as u8 + 2) * 25
            && entries[channel * 4 + 1] == channel as u8
    }) {
        return Err(ReadError::Invalid);
    }
    let mut mapped = vec![
        (driver.start, driver.start + driver.profile.len),
        (table_offset, table_offset + 16),
    ];
    let mut tracks = Vec::new();
    for channel in 0..4 {
        let pointer = word(entries, channel * 4 + 2);
        let note_count =
            sequence::validate(bytes, bank, pointer, channel as u8, &mut mapped, budget)?;
        tracks.push(GbToseTrack {
            number: channel as u8 + 1,
            note_count,
        });
    }
    if tracks.iter().all(|track| track.note_count == 0) {
        return Err(ReadError::Invalid);
    }
    Ok(GbToseSong {
        profile: driver.profile.name,
        index,
        title: format!("Song {index}"),
        bank,
        hardware: GbToseHardware::Dmg,
        table_entry: span(table_offset, 16),
        tracks,
        mapped_spans: spans(mapped),
        warnings: Vec::new(),
    })
}

fn checked_driver(
    bytes: &[u8],
    expected: &GbToseSong,
    cancel: &AtomicBool,
) -> anyhow::Result<profiles::Driver> {
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let drivers = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Game Boy TOSE validation stopped: {stop:?}"))?;
    for driver in drivers {
        if driver.profile.name == expected.profile
            && expected.index <= 252
            && expected.bank != 0
            && !expected.bank.is_multiple_of(32)
            && usize::from(expected.bank) < bytes.len() / 0x4000
            && let Ok(checked) = song(bytes, driver, expected.bank, expected.index, &mut budget)
            && checked == *expected
        {
            return Ok(driver);
        }
    }
    anyhow::bail!("Game Boy TOSE inventory differs from its recognized source")
}

pub fn validate_song(bytes: &[u8], song: &GbToseSong, cancel: &AtomicBool) -> anyhow::Result<()> {
    checked_driver(bytes, song, cancel).map(|_| ())
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn span(offset: usize, len: usize) -> RomSpan {
    RomSpan {
        effective_offset: offset as u32,
        byte_len: len as u32,
        canonical_cpu_address: if offset < 0x4000 {
            offset
        } else {
            0x4000 + offset % 0x4000
        } as u32,
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

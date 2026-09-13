use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop};

mod macros;
mod native;
mod profiles;
mod project;
mod sequence;
#[cfg(any(test, feature = "test-support"))]
mod tests;

pub use native::prepare_rom;
#[cfg(feature = "test-support")]
pub use tests::synthetic_rom;

#[derive(Debug)]
enum ReadError {
    Invalid(&'static str),
    Stop(ScanStop),
}

impl From<ScanStop> for ReadError {
    fn from(value: ScanStop) -> Self {
        Self::Stop(value)
    }
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(reason) => f.write_str(reason),
            Self::Stop(reason) => write!(f, "Game Boy MusyX validation stopped: {reason:?}"),
        }
    }
}

fn require(valid: bool, reason: &'static str) -> Result<(), ReadError> {
    valid.then_some(()).ok_or(ReadError::Invalid(reason))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbMusyxSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub bank: u8,
    pub table_entry: RomSpan,
    pub header: RomSpan,
    pub tracks: Vec<GbMusyxTrack>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbMusyxTrack {
    pub number: u8,
    pub note_count: u32,
}

pub struct PreparedGbMusyx {
    pub bytes: Vec<u8>,
    pub ready_address: u16,
    pub ready_value: u8,
    pub ack_address: u16,
    pub ack_value: u8,
    pub wait_start: u16,
    pub wait_end: u16,
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbMusyxSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    for driver in profiles::recognized(bytes, budget)? {
        let project = match project::Project::new(bytes, driver, budget) {
            Ok(project) => project,
            Err(ReadError::Invalid(_)) => continue,
            Err(ReadError::Stop(stop)) => return Err(stop),
        };
        for index in 0..project.song_count {
            budget.charge()?;
            match project.song(index, budget) {
                Ok(song) => {
                    if songs.len() >= remaining {
                        return Err(ScanStop::CandidateLimit);
                    }
                    songs.push(song);
                }
                Err(ReadError::Invalid(_)) => (),
                Err(ReadError::Stop(stop)) => return Err(stop),
            }
        }
    }
    Ok(())
}

fn checked_driver(
    bytes: &[u8],
    song: &GbMusyxSong,
    cancel: &AtomicBool,
) -> anyhow::Result<profiles::Driver> {
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let drivers = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!(ReadError::Stop(stop)))?;
    for driver in drivers {
        if driver.profile.name != song.profile {
            continue;
        }
        let project = match project::Project::new(bytes, driver, &mut budget) {
            Ok(project) => project,
            Err(ReadError::Invalid(_)) => continue,
            Err(error) => return Err(anyhow::anyhow!(error)),
        };
        let checked = match project.song(usize::from(song.index), &mut budget) {
            Ok(checked) => checked,
            Err(ReadError::Invalid(_)) => continue,
            Err(error) => return Err(anyhow::anyhow!(error)),
        };
        if checked == *song {
            return Ok(driver);
        }
    }
    anyhow::bail!("Game Boy MusyX inventory differs from its recognized source")
}

pub fn validate_song(bytes: &[u8], song: &GbMusyxSong, cancel: &AtomicBool) -> anyhow::Result<()> {
    checked_driver(bytes, song, cancel).map(|_| ())
}

fn spans(ranges: impl IntoIterator<Item = (usize, usize)>) -> Vec<RomSpan> {
    let mut pieces = Vec::new();
    for (mut start, end) in ranges {
        while start < end {
            let stop = end.min((start / 0x4000 + 1) * 0x4000);
            pieces.push((start, stop));
            start = stop;
        }
    }
    pieces.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in pieces {
        if let Some(last) = merged.last_mut()
            && last.0 / 0x4000 == start / 0x4000
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
        .into_iter()
        .map(|(start, end)| span(start, end - start))
        .collect()
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

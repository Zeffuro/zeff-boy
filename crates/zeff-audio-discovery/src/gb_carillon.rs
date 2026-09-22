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
pub use tests::{synthetic_rom, synthetic_rom_alternate};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbCarillonSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub bank: u16,
    pub order_address: u16,
    pub aliases: Vec<u16>,
    pub table_entry: RomSpan,
    pub tracks: Vec<GbCarillonTrack>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbCarillonTrack {
    pub number: u8,
    pub note_count: u32,
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
    songs: &mut Vec<GbCarillonSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    for recognized in profiles::recognized(bytes, budget)? {
        for index in 0..8 {
            match sequence::song(bytes, recognized, index, budget) {
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

pub fn validate_song(
    bytes: &[u8],
    expected: &GbCarillonSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let banks = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Carillon validation stopped: {stop:?}"))?;
    let recognized = banks
        .iter()
        .copied()
        .find(|recognized| {
            recognized.bank == expected.bank && recognized.profile.name == expected.profile
        })
        .ok_or_else(|| anyhow::anyhow!("Carillon inventory differs from its recognized source"))?;
    anyhow::ensure!(
        profiles::named(expected.profile).is_some()
            && sequence::song(bytes, recognized, expected.index, &mut budget)
                .ok()
                .as_ref()
                == Some(expected),
        "Carillon inventory differs from its recognized source"
    );
    Ok(())
}

fn span(bank: u16, address: u16, len: usize) -> RomSpan {
    RomSpan {
        effective_offset: u32::from(bank) * 0x4000 + u32::from(address) - 0x4000,
        byte_len: len as u32,
        canonical_cpu_address: u32::from(address),
    }
}

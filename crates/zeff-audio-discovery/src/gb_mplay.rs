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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbMplaySong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub bank: u16,
    pub order_address: u16,
    pub table_entry: RomSpan,
    pub tracks: Vec<GbMplayTrack>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbMplayTrack {
    pub number: u8,
    pub note_count: u32,
}

pub fn supports_cartridge(bytes: &[u8]) -> bool {
    super::gb_carillon::supports_cartridge(bytes)
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
    songs: &mut Vec<GbMplaySong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    for recognized in profiles::recognized(bytes, budget)? {
        for index in 0..recognized.profile.selector_count {
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
    expected: &GbMplaySong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let banks = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("MPlay validation stopped: {stop:?}"))?;
    let recognized = banks
        .into_iter()
        .find(|r| r.bank == expected.bank && r.profile.name == expected.profile)
        .ok_or_else(|| anyhow::anyhow!("MPlay inventory differs from its recognized source"))?;
    anyhow::ensure!(
        sequence::song(bytes, recognized, expected.index, &mut budget)
            .ok()
            .as_ref()
            == Some(expected),
        "MPlay inventory differs from its recognized source"
    );
    Ok(())
}

fn span(bank: u16, address: u16, len: usize) -> RomSpan {
    RomSpan {
        effective_offset: if bank == 0 {
            u32::from(address)
        } else {
            u32::from(bank) * 0x4000 + u32::from(address) - 0x4000
        },
        byte_len: len as u32,
        canonical_cpu_address: u32::from(address),
    }
}

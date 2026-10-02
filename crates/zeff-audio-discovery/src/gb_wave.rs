use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop};

mod native;
mod profiles;
#[cfg(any(test, feature = "test-support"))]
mod tests;

pub use native::prepare_rom;
#[cfg(feature = "test-support")]
pub use tests::{synthetic_frame_rom, synthetic_rom};

pub const MAX_PREVIEW_SECONDS: u32 = 180;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbWaveSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub bank: u16,
    pub header_address: u16,
    pub table_entry: RomSpan,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

pub fn supports_cartridge(bytes: &[u8]) -> bool {
    bytes.len() >= 0x8000
        && matches!(bytes[0x143], 0x80 | 0xc0)
        && matches!(bytes[0x147], 0x19..=0x1e)
        && bytes[0x148] <= 8
        && bytes.len() == (0x8000usize << bytes[0x148])
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbWaveSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = profiles::recognized(bytes, budget)? else {
        return Ok(());
    };
    budget.charge()?;
    if songs.len() >= remaining {
        return Err(ScanStop::CandidateLimit);
    }
    songs.push(song(profile));
    Ok(())
}

fn song(profile: &profiles::Profile) -> GbWaveSong {
    GbWaveSong {
        profile: profile.name,
        index: u16::from(profile.index),
        title: format!("Selection {:03}", profile.index),
        bank: u16::from(profile.bank),
        header_address: profile.header,
        table_entry: span(profile.bank, profile.entry, 2),
        mapped_spans: profile.spans.iter().map(|&(bank, address, len)| span(bank, address, len)).collect(),
        warnings: vec![
            "Exact source-bound banked wave-state selection; other revisions and selectors require separate qualification.".into(),
            if profile.timer {
                "Runs the original sequencer in CGB double speed on every fourth native timer overflow; original APU phase and gameplay-driven sound changes are not reproduced.".into()
            } else {
                "Runs the original sequencer in CGB double speed once per hardware frame; original mainloop latency, APU phase and gameplay-driven sound changes are not reproduced.".into()
            },
            "Music/effect roles, natural duration and complete soundtrack coverage are unknown.".into(),
        ],
    }
}

pub fn validate_song(
    bytes: &[u8],
    expected: &GbWaveSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Wave-state validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Wave-state source differs from its qualified profile"))?;
    anyhow::ensure!(
        song(profile) == *expected,
        "Wave-state selection differs from its source-bound inventory"
    );
    Ok(())
}

fn span(bank: u8, address: u16, len: u16) -> RomSpan {
    RomSpan {
        effective_offset: if bank == 0 {
            u32::from(address)
        } else {
            u32::from(bank) * 0x4000 + u32::from(address) - 0x4000
        },
        byte_len: u32::from(len),
        canonical_cpu_address: u32::from(address),
    }
}

use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop};

mod native;
mod profiles;
#[cfg(any(test, feature = "test-support"))]
mod tests;

pub use native::prepare_rom;
#[cfg(feature = "test-support")]
pub use tests::synthetic_rom;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbTimedSong {
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
        && bytes[0x143] == 0x80
        && matches!(bytes[0x147], 0x19..=0x1e)
        && bytes[0x148] <= 6
        && bytes.len() == (0x8000usize << bytes[0x148])
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbTimedSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = profiles::recognized(bytes, budget)? else {
        return Ok(());
    };
    for selection in profile.selections {
        budget.charge()?;
        if songs.len() >= remaining {
            return Err(ScanStop::CandidateLimit);
        }
        songs.push(song(profile, selection));
    }
    Ok(())
}

fn song(profile: &profiles::Profile, selection: &profiles::Selection) -> GbTimedSong {
    GbTimedSong {
        profile: profile.name,
        index: selection.index,
        title: format!("Selection {:03}", selection.index),
        bank: selection.bank,
        header_address: selection.address,
        table_entry: span(selection.caller_bank, selection.caller, 8),
        mapped_spans: selection
            .spans
            .iter()
            .map(|&(bank, address, length)| span(bank, address, length))
            .collect(),
        warnings: vec![
            "Exact source-bound timed event selection; other revisions and driver builds require separate qualification.".into(),
            "Qualified only for DMG hardware; runs both original sequencer services once per hardware frame; original DIV/APU phase, graphics interrupt latency, missed gameplay frames and SFX mixing are not reproduced.".into(),
            "Original end commands restart the source default selection; music/effect roles, natural duration and complete soundtrack coverage are unknown.".into(),
        ],
    }
}

pub fn validate_song(
    bytes: &[u8],
    expected: &GbTimedSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Timed validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Timed source differs from its qualified profile"))?;
    let selection = profile
        .selections
        .iter()
        .find(|selection| selection.index == expected.index)
        .ok_or_else(|| anyhow::anyhow!("Timed selection is outside its qualified domain"))?;
    anyhow::ensure!(
        song(profile, selection) == *expected,
        "Timed selection differs from its source-bound inventory"
    );
    Ok(())
}

fn span(bank: u16, address: u16, length: u16) -> RomSpan {
    RomSpan {
        effective_offset: if bank == 0 {
            u32::from(address)
        } else {
            u32::from(bank) * 0x4000 + u32::from(address) - 0x4000
        },
        byte_len: u32::from(length),
        canonical_cpu_address: u32::from(address),
    }
}

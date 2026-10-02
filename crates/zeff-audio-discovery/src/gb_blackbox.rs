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
pub struct GbBlackBoxSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub bank: u16,
    pub module_address: u16,
    pub initial_order: u8,
    pub loop_order: u8,
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
    songs: &mut Vec<GbBlackBoxSong>,
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

fn song(profile: &profiles::Profile, selection: &profiles::Selection) -> GbBlackBoxSong {
    GbBlackBoxSong {
        profile: profile.name,
        index: selection.index,
        title: format!("Selection {:03}", selection.index),
        bank: selection.bank,
        module_address: selection.module,
        initial_order: selection.initial,
        loop_order: selection.restart,
        table_entry: span(selection.entry_bank, selection.entry, selection.entry_len),
        mapped_spans: vec![
            span(0, profile.init - 0x90, 0x415),
            span(selection.bank, selection.module, selection.data_len),
        ],
        warnings: vec![
            "Exact source-bound Black Box Music Box selection; other revisions and driver builds require separate qualification.".into(),
            "Runs the original sequencer at one tick per CGB double-speed hardware frame; game-driven DIV resets, APU frame-sequencer phase and gameplay transitions are not reproduced.".into(),
            "Music/effect roles, natural duration and complete soundtrack coverage are unknown.".into(),
        ],
    }
}

pub fn validate_song(
    bytes: &[u8],
    expected: &GbBlackBoxSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Black Box validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Black Box source differs from its qualified profile"))?;
    let selection = profile
        .selections
        .iter()
        .find(|selection| selection.index == expected.index)
        .ok_or_else(|| anyhow::anyhow!("Black Box selection is outside its qualified domain"))?;
    anyhow::ensure!(
        song(profile, selection) == *expected,
        "Black Box selection differs from its source-bound inventory"
    );
    Ok(())
}

fn span(bank: u16, address: u16, len: u16) -> RomSpan {
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

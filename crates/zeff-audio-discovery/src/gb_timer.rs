use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop};

mod native;
mod profiles;
#[cfg(any(test, feature = "test-support"))]
mod tests;

pub use native::prepare_rom;
#[cfg(feature = "test-support")]
pub use tests::{synthetic_mbc2_rom, synthetic_rom};

pub const MAX_PREVIEW_SECONDS: u32 = 180;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbTimerSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub compressed: bool,
    pub module_offset: u32,
    pub table_entry: RomSpan,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

pub fn supports_cartridge(bytes: &[u8]) -> bool {
    if bytes.len() < 0x8000 || bytes[0x143] != 0 || bytes[0x148] > 6 {
        return false;
    }
    let mapper_matches = match bytes[0x147] {
        3 => bytes[0x149] <= 3,
        6 => bytes[0x148] <= 3 && bytes[0x149] == 0,
        _ => false,
    };
    mapper_matches && bytes.len() == (0x8000usize << bytes[0x148])
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbTimerSong>,
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

fn song(profile: &profiles::Profile, selection: &profiles::Selection) -> GbTimerSong {
    GbTimerSong {
        profile: profile.name,
        index: selection.index,
        title: format!("Selection {:03}", selection.index),
        compressed: profile.compressed,
        module_offset: selection.module,
        table_entry: span(selection.entry, 2),
        mapped_spans: selection
            .spans
            .iter()
            .map(|&(offset, len)| span(offset, len))
            .collect(),
        warnings: vec![
            "Exact source-bound timer driver selection; other sources and selector domains require separate qualification.".into(),
            "Preserves native decompression and dynamic timer periods; original gameplay/SFX mixing and DIV/APU/interrupt phase are not reproduced.".into(),
            "Preview/export is qualified for at most 180 seconds; music/effect roles, natural duration and complete soundtrack coverage remain unknown.".into(),
        ],
    }
}

pub fn validate_song(
    bytes: &[u8],
    expected: &GbTimerSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Timer driver validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Timer driver source differs from its qualified profile"))?;
    let selection = profile
        .selections
        .iter()
        .find(|selection| selection.index == expected.index)
        .ok_or_else(|| anyhow::anyhow!("Timer driver selector is outside its qualified domain"))?;
    anyhow::ensure!(
        song(profile, selection) == *expected,
        "Timer driver selection differs from its source-bound inventory"
    );
    Ok(())
}

fn span(offset: u32, len: u32) -> RomSpan {
    RomSpan {
        effective_offset: offset,
        byte_len: len,
        canonical_cpu_address: if offset < 0x4000 {
            offset
        } else {
            0x4000 + (offset & 0x3fff)
        },
    }
}

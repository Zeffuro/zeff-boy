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

pub const MAX_PREVIEW_SECONDS: u32 = 180;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbCacheSong {
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
        && bytes[0x143] == 0
        && bytes[0x147] == 1
        && bytes[0x148] <= 4
        && bytes[0x149] == 0
        && bytes.len().is_power_of_two()
        && bytes.len() <= 0x80000
        && bytes.len() >= (0x8000usize << bytes[0x148])
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbCacheSong>,
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

fn song(profile: &profiles::Profile) -> GbCacheSong {
    GbCacheSong {
        profile: profile.name,
        index: profile.index,
        title: format!("Selection {:03}", profile.index),
        bank: profile.bank,
        header_address: profile.api,
        table_entry: span(u32::from(profile.caller), 3),
        mapped_spans: profile.spans.iter().map(|&(offset, len)| span(offset, len)).collect(),
        warnings: vec![
            "Exact source-bound cached-register selection; other revisions and driver builds require separate qualification.".into(),
            "Preserves original cold initialization until the qualified selector caller, then runs the native service once per DMG hardware frame; original gameplay interrupts, SFX mixing and DIV/APU phase are not reproduced.".into(),
            "One original caller selection is qualified; music/effect roles, natural duration and complete soundtrack coverage are unknown.".into(),
        ],
    }
}

pub fn validate_song(
    bytes: &[u8],
    expected: &GbCacheSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Cache validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Cache source differs from its qualified profile"))?;
    anyhow::ensure!(
        song(profile) == *expected,
        "Cache selection differs from its source-bound inventory"
    );
    Ok(())
}

fn span(offset: u32, length: u32) -> RomSpan {
    RomSpan {
        effective_offset: offset,
        byte_len: length,
        canonical_cpu_address: if offset < 0x4000 {
            offset
        } else {
            0x4000 + offset % 0x4000
        },
    }
}

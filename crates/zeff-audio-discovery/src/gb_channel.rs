use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop};

mod native;
mod profiles;
#[cfg(any(test, feature = "test-support"))]
mod tests;

pub use native::prepare_rom;
pub const MAX_PREVIEW_SECONDS: u32 = 180;
#[cfg(feature = "test-support")]
pub use tests::synthetic_rom;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbChannelSong {
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
        && bytes[0x148] <= 7
        && bytes.len() == (0x8000usize << bytes[0x148])
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbChannelSong>,
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

fn song(profile: &profiles::Profile) -> GbChannelSong {
    GbChannelSong {
        profile: profile.name,
        index: profile.index,
        title: format!("Selection {:03}", profile.index),
        bank: 7,
        header_address: (profile.header % 0x4000 + 0x4000) as u16,
        table_entry: span(profile.table, 2),
        mapped_spans: profile.spans.iter().map(|&(offset, length)| span(offset, length)).collect(),
        warnings: vec![
            "Exact source-bound channel selection; other selectors and revisions require separate qualification.".into(),
            "DMG only; original timer ISR, VBlank timer rearm and native RAM helper generation are preserved; original gameplay, DIV/APU phase and interrupt latency are not reproduced.".into(),
            "Music/effect roles, natural duration, gameplay SFX mixing and complete soundtrack coverage are unknown.".into(),
        ],
    }
}

pub fn validate_song(
    bytes: &[u8],
    expected: &GbChannelSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Channel validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Channel source differs from its qualified profile"))?;
    anyhow::ensure!(
        song(profile) == *expected,
        "Channel selection differs from its source-bound inventory"
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
            offset % 0x4000 + 0x4000
        },
    }
}

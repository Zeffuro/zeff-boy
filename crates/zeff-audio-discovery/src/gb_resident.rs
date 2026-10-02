use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop};

mod native;
mod profiles;
#[cfg(any(test, feature = "test-support"))]
mod tests;

pub use native::prepare_rom;
#[cfg(feature = "test-support")]
pub use tests::{synthetic_rom, synthetic_timer_rom};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbResidentSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub bank: u16,
    pub header_address: u16,
    pub timer_modulo: Option<u8>,
    pub table_entry: RomSpan,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

pub fn supports_cartridge(bytes: &[u8]) -> bool {
    bytes.len() >= 0x8000
        && bytes[0x143] == 0
        && bytes[0x147] == 1
        && bytes[0x148] <= 6
        && bytes.len() == (0x8000usize << bytes[0x148])
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbResidentSong>,
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

fn song(profile: &profiles::Profile, selection: &profiles::Selection) -> GbResidentSong {
    GbResidentSong {
        profile: profile.name,
        index: selection.index,
        title: format!("Selection {:03}", selection.index),
        bank: profile.bank,
        header_address: selection.header,
        timer_modulo: profile.timer_modulo,
        table_entry: span(0, profile.table + selection.index, 2),
        mapped_spans: vec![
            span(0, profile.init, profile.resident_len),
            span(profile.bank, selection.header, selection.data_len),
        ],
        warnings: vec![
            "Exact source-bound resident four-channel selection; other revisions and driver builds require separate qualification.".into(),
            if profile.timer_modulo.is_some() {
                "Runs the original sequencer at the source timer period of 73728 DMG T-cycles; original DIV/APU phase and gameplay transitions are not reproduced.".into()
            } else {
                "Runs the original sequencer once per DMG hardware frame; original DIV/APU phase, graphics interrupt latency and gameplay transitions are not reproduced.".into()
            },
            "Music/effect roles, natural duration and complete soundtrack coverage are unknown.".into(),
        ],
    }
}

pub fn validate_song(
    bytes: &[u8],
    expected: &GbResidentSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Resident validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Resident source differs from its qualified profile"))?;
    let selection = profile
        .selections
        .iter()
        .find(|selection| selection.index == expected.index)
        .ok_or_else(|| anyhow::anyhow!("Resident selection is outside its qualified domain"))?;
    anyhow::ensure!(
        song(profile, selection) == *expected,
        "Resident selection differs from its source-bound inventory"
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

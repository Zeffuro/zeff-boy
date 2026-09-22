use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result as AnyResult, ensure};
use serde::Serialize;

use crate::{Budget, MediaIdentity, RomSpan, ScanStop, SourceSpan};

mod bootstrap;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
mod foreground;
mod nintendo;
mod presets;
mod profiles;
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use fixture::{fixture_rom, fixture_rom_pc10};
#[cfg(any(test, feature = "test-support"))]
pub use foreground::fixture_rom as fixture_rom_foreground;
#[cfg(any(test, feature = "test-support"))]
pub use nintendo::fixture_rom as fixture_rom_nintendo;
#[cfg(any(test, feature = "test-support"))]
pub use presets::fixture_rom as fixture_rom_presets;
#[cfg(any(test, feature = "test-support"))]
pub use presets::fixture_rom_cnrom as fixture_rom_presets_cnrom;
use profiles::{Profile, all_profiles};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NesNativeTiming {
    Ntsc,
    Pal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesNativeSong {
    pub profile: &'static str,
    pub index: u16,
    pub raw_index: u8,
    pub title: String,
    pub header: RomSpan,
    pub table_entry: RomSpan,
    pub channels: Vec<NesNativeChannel>,
    pub native: NesNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesNativeChannel {
    pub number: u8,
    pub entry: RomSpan,
    pub sequence: RomSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesNativeProfile {
    pub mapper: u16,
    pub timing: NesNativeTiming,
    pub init: RomSpan,
    pub tick: RomSpan,
    pub driver: RomSpan,
    pub tables: RomSpan,
    pub bootstrap: RomSpan,
}

pub struct PreparedNesNative {
    pub bytes: Vec<u8>,
    pub mapper: u16,
    pub timing: NesNativeTiming,
    pub ready_address: u16,
    pub ack_address: u16,
    pub wait_start: u16,
    pub wait_end: u16,
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<NesNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        nintendo::scan(bytes, songs, budget, max_candidates)?;
        foreground::scan(bytes, songs, budget, max_candidates)?;
        return presets::scan(bytes, songs, budget, max_candidates);
    };
    for index in 0..profile.cues.len() {
        budget.charge()?;
        if songs.len() >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        for _ in profile.cues[index].streams {
            budget.charge()?;
        }
        songs.push(inspect(bytes, profile, index).ok_or(ScanStop::ValidationLimit)?);
    }
    Ok(())
}

pub fn prepare_rom(
    bytes: &[u8],
    song: &NesNativeSong,
    cancel: &AtomicBool,
) -> AnyResult<PreparedNesNative> {
    if presets::owns(song.profile) {
        return presets::prepare(bytes, song, cancel);
    }
    if foreground::owns(song.profile) {
        return foreground::prepare(bytes, song, cancel);
    }
    if nintendo::owns(song.profile) {
        return nintendo::prepare(bytes, song, cancel);
    }
    let mut budget = Budget {
        cancel,
        remaining: 1_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("NES native validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("NES source no longer matches its native profile"))?;
    ensure!(
        inspect(bytes, profile, usize::from(song.index)).as_ref() == Some(song),
        "NES native selection no longer matches its source"
    );
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "NES native preparation cancelled"
    );
    bootstrap::build(bytes, song)
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    if !matches!(bytes.len(), 0x10010 | 0x12010)
        || bytes.get(..6) != Some(b"NES\x1a\x02\x04")
        || bytes[6] != 0x31
        || !matches!(bytes[7], 0 | 2)
    {
        return Ok(None);
    }
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    let hash = zeff_firmware::sha256_hex(bytes);
    Ok(all_profiles().find(|profile| profile.sources.contains(&(hash.as_str(), bytes.len()))))
}

fn inspect(bytes: &[u8], profile: &Profile, index: usize) -> Option<NesNativeSong> {
    let cue = profile.cues.get(index)?;
    let table_address = 0xeffb_u16.checked_add(u16::from(cue.raw & 0x3f) * 3)?;
    let header = prg_span(bytes, table_address, cue.streams.len() * 3)?;
    let mut mapped = vec![
        prg_span(bytes, 0xec4c, 0x39a)?,
        prg_span(bytes, 0xefe6, usize::from(profile.tables_end - 0xefe6))?,
    ];
    let mut channels = Vec::with_capacity(cue.streams.len());
    for (channel, &(start, end)) in cue.streams.iter().enumerate() {
        let entry = prg_span(bytes, table_address.checked_add(channel as u16 * 3)?, 3)?;
        let offset = entry.effective_offset as usize;
        let row = bytes.get(offset..offset + 3)?;
        if row[0] != channel as u8 * 4
            || u16::from_le_bytes([row[1], row[2]]) != start
            || end <= start
        {
            return None;
        }
        let sequence = prg_span(bytes, start, usize::from(end - start))?;
        mapped.push(sequence);
        channels.push(NesNativeChannel {
            number: channel as u8 + 1,
            entry,
            sequence,
        });
    }
    for &(start, end) in cue.extra {
        mapped.push(prg_span(
            bytes,
            start,
            usize::from(end.checked_sub(start)?),
        )?);
    }
    let native = NesNativeProfile {
        mapper: 3,
        timing: NesNativeTiming::Ntsc,
        init: prg_span(bytes, 0xec4c, 0x94)?,
        tick: prg_span(bytes, 0xed30, 0x2a5)?,
        driver: mapped[0],
        tables: mapped[1],
        bootstrap: prg_span(bytes, profile.bootstrap, 128)?,
    };
    mapped.sort_unstable();
    mapped.dedup();
    if mapped
        .iter()
        .any(|span| intersects(*span, native.bootstrap))
    {
        return None;
    }
    Some(NesNativeSong {
        profile: profile.id,
        index: index as u16,
        raw_index: cue.raw,
        title: format!("Native audio selector {:02X}", cue.raw),
        header,
        table_entry: header,
        channels,
        native,
        mapped_spans: mapped,
        warnings: vec![
            "Only the listed native audio selectors are qualified; cues and effects may be included and full soundtrack coverage is not established.".into(),
            "Mapped original driver, table and sequence ranges are preserved; native commands are not projected to MIDI or instrument programs.".into(),
        ],
    })
}

fn prg_span(bytes: &[u8], address: u16, len: usize) -> Option<RomSpan> {
    let offset = usize::from(address.checked_sub(0x8000)?) + 16;
    let end = offset.checked_add(len)?;
    (len != 0 && end <= 0x8010 && end <= bytes.len()).then_some(RomSpan {
        effective_offset: offset as u32,
        byte_len: len as u32,
        canonical_cpu_address: u32::from(address),
    })
}

fn intersects(a: RomSpan, b: RomSpan) -> bool {
    a.effective_offset < b.effective_offset + b.byte_len
        && b.effective_offset < a.effective_offset + a.byte_len
}

pub fn source_span_matches(media: &MediaIdentity, span: SourceSpan) -> bool {
    foreground::source_span_matches(media, span)
        || nintendo::source_span_matches(media, span)
        || presets::source_span_matches(media, span)
        || (media.system == "nes"
            && matches!(media.byte_len, 0x10010 | 0x12010)
            && media.sha256.as_deref().is_some_and(|hash| {
                all_profiles()
                    .any(|profile| profile.sources.contains(&(hash, media.byte_len as usize)))
            })
            && span.byte_len != 0
            && span.effective_offset >= 16
            && u64::from(span.effective_offset) + u64::from(span.byte_len) <= 0x8010
            && span.canonical_cpu_address == Some(0x8000 + span.effective_offset - 16))
}

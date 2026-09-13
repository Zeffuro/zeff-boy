use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result as AnyResult, ensure};
use serde::Serialize;

use crate::{Budget, MediaIdentity, RomSpan, ScanStop, SourceSpan};

mod bootstrap;
mod cgb_banked;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
mod profiles;
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use cgb_banked::fixture::fixture_rom as cgb_fixture_rom;
#[cfg(any(test, feature = "test-support"))]
pub use cgb_banked::fixture::ram_fixture_rom as cgb_ram_fixture_rom;
#[cfg(any(test, feature = "test-support"))]
pub use fixture::{expanded_fixture_rom, fixture_rom};
use profiles::{Profile, all_profiles};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GbNativeTiming {
    Dmg,
    CgbDouble,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbNativeSong {
    pub profile: &'static str,
    pub index: u16,
    pub raw_index: u8,
    pub title: String,
    pub bank: u8,
    pub header: RomSpan,
    pub table_entry: RomSpan,
    pub channels: Vec<GbNativeChannel>,
    pub native: GbNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub playback_frames: u32,
    // CPU T-clocks from the playback handoff at the profile's qualified speed.
    pub playback_clocks: u64,
    pub loop_start_frame: Option<u32>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbNativeChannel {
    pub number: u8,
    pub entry: RomSpan,
    pub sequence: RomSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbNativeProfile {
    pub cartridge_type: u8,
    pub timing: GbNativeTiming,
    pub init: RomSpan,
    pub tick: RomSpan,
    pub driver: RomSpan,
    pub tables: RomSpan,
    pub bootstrap: RomSpan,
    pub startup_hook: RomSpan,
}

pub struct PreparedGbNative {
    pub bytes: Vec<u8>,
    pub timing: GbNativeTiming,
    pub ready_address: u16,
    pub ready_value: u8,
    pub ack_address: u16,
    pub ack_value: u8,
    pub wait_start: u16,
    pub wait_end: u16,
    pub playback_frames: u32,
    pub playback_clocks: u64,
}

pub(crate) fn detect_drivers(
    bytes: &[u8],
    source_sha256: Option<&str>,
    findings: &mut Vec<crate::drivers::DriverFinding>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    cgb_banked::detection::scan(bytes, source_sha256, findings, budget, max_candidates)
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    if cgb_banked::candidate(bytes) {
        return cgb_banked::scan(bytes, songs, budget, max_candidates);
    }
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(());
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
    song: &GbNativeSong,
    cancel: &AtomicBool,
) -> AnyResult<PreparedGbNative> {
    if cgb_banked::candidate(bytes) {
        return cgb_banked::prepare_rom(bytes, song, cancel);
    }
    let mut budget = Budget {
        cancel,
        remaining: 1_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("GB native validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("GB source no longer matches its native profile"))?;
    ensure!(
        inspect(bytes, profile, usize::from(song.index)).as_ref() == Some(song),
        "GB native selection no longer matches its source"
    );
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "GB native preparation cancelled"
    );
    bootstrap::build(bytes, song)
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    if bytes.len() != 0x10_0000
        || bytes.get(0x143) != Some(&0)
        || bytes[0x147..0x14a] != [0x13, 5, 3]
    {
        return Ok(None);
    }
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    let hash = zeff_firmware::sha256_hex(bytes);
    Ok(all_profiles().find(|profile| profile.sources.contains(&hash.as_str())))
}

fn inspect(bytes: &[u8], profile: &Profile, index: usize) -> Option<GbNativeSong> {
    let cue = profile.cues.get(index)?;
    let address = 0x4000_u16.checked_add(u16::from(cue.raw) * 3)?;
    let header = rom_span(bytes, 2, address, cue.streams.len() * 3)?;
    let mut channels = Vec::with_capacity(cue.streams.len());
    for (channel, &(start, end)) in cue.streams.iter().enumerate() {
        let entry = rom_span(bytes, 2, address.checked_add(channel as u16 * 3)?, 3)?;
        let offset = entry.effective_offset as usize;
        let row = bytes.get(offset..offset + 3)?;
        let expected = channel as u8
            | if channel == 0 {
                (cue.streams.len() as u8 - 1) << 6
            } else {
                0
            };
        if row[0] != expected || u16::from_le_bytes([row[1], row[2]]) != start || end <= start {
            return None;
        }
        channels.push(GbNativeChannel {
            number: channel as u8 + 1,
            entry,
            sequence: rom_span(bytes, 2, start, usize::from(end - start))?,
        });
    }
    let native = GbNativeProfile {
        cartridge_type: 0x13,
        timing: GbNativeTiming::Dmg,
        init: rom_span(bytes, 0, 0x200e, 0x16)?,
        tick: rom_span(bytes, 2, 0x5103, 0x35)?,
        driver: rom_span(bytes, 2, 0x5103, 0xa44)?,
        tables: rom_span(bytes, 2, 0x4000, 0x2fd)?,
        bootstrap: rom_span(bytes, 0, 0x3fb0, 0x50)?,
        startup_hook: rom_span(bytes, 0, 0x1fd3, 3)?,
    };
    let mapped_spans = vec![
        native.init,
        rom_span(bytes, 0, 0x23a1, 0x88)?,
        rom_span(bytes, 0, 0x28cb, 0x53)?,
        rom_span(bytes, 2, 0x4000, 0x4000)?,
    ];
    Some(GbNativeSong {
        profile: profile.id,
        index: index as u16,
        raw_index: cue.raw,
        title: format!("Native music selector {:02X}", cue.raw),
        bank: 2,
        header,
        table_entry: header,
        channels,
        native,
        mapped_spans,
        playback_frames: cue.playback_frames,
        playback_clocks: *profile.playback_clocks.get(index)?,
        loop_start_frame: cue.loop_start_frame,
        warnings: vec![
            "Only the listed music groups in bank 2 are qualified; other music banks and sound effects are not enumerated.".into(),
            "The complete original audio bank is retained for shared drum streams and waveform data; native commands are not projected to MIDI or instrument programs.".into(),
        ],
    })
}

fn rom_span(bytes: &[u8], bank: u8, address: u16, len: usize) -> Option<RomSpan> {
    let offset = if bank == 0 && address < 0x4000 {
        usize::from(address)
    } else if bank > 0 && (0x4000..0x8000).contains(&address) {
        usize::from(bank) * 0x4000 + usize::from(address - 0x4000)
    } else {
        return None;
    };
    (len != 0 && len <= 0x4000 - offset % 0x4000 && offset.checked_add(len)? <= bytes.len())
        .then_some(RomSpan {
            effective_offset: offset as u32,
            byte_len: len as u32,
            canonical_cpu_address: u32::from(address),
        })
}

pub fn source_span_matches(media: &MediaIdentity, span: SourceSpan) -> bool {
    if cgb_banked::source_matches(media) {
        return valid_source_span(media.byte_len, span);
    }
    if !matches!(media.system, "gb" | "gbc")
        || media.byte_len != 0x10_0000
        || !media
            .sha256
            .as_deref()
            .is_some_and(|hash| all_profiles().any(|profile| profile.sources.contains(&hash)))
    {
        return false;
    }
    valid_source_span(media.byte_len, span)
}

fn valid_source_span(byte_len: u64, span: SourceSpan) -> bool {
    let offset = span.effective_offset;
    let within_bank = offset % 0x4000;
    span.byte_len != 0
        && u64::from(offset) < byte_len
        && span.byte_len <= 0x4000 - within_bank
        && span.canonical_cpu_address
            == Some(if offset < 0x4000 {
                offset
            } else {
                0x4000 + within_bank
            })
}

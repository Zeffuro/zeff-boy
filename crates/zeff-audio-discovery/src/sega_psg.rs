use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result as AnyResult, ensure};
use serde::Serialize;
use zeff_emu_common::system::System;

use super::{Budget, MediaIdentity, RomSpan, ScanStop, SourceSpan};

mod bootstrap;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
mod profiles;
#[cfg(test)]
mod supplemental_tests;
#[cfg(test)]
mod tests;

pub use bootstrap::PreparedSegaPsg;
#[cfg(any(test, feature = "test-support"))]
pub use fixture::{fixture_rom, fixture_rom_six_byte, fixture_rom_supplemental};
use profiles::{HeaderLayout, Profile, all_profiles};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SegaPsgRegion {
    Japanese,
    Export,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SegaPsgTiming {
    Ntsc,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SegaPsgSong {
    pub profile: &'static str,
    #[serde(serialize_with = "serialize_system")]
    pub system: System,
    pub region: SegaPsgRegion,
    pub timing: SegaPsgTiming,
    pub frame_divider: u8,
    pub index: u16,
    pub raw_index: u8,
    pub title: String,
    pub table_entry: RomSpan,
    pub header: RomSpan,
    pub driver: RomSpan,
    pub channels: Vec<SegaPsgChannel>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SegaPsgChannel {
    pub number: u8,
    pub entry: RomSpan,
    pub sequence: RomSpan,
    pub cpu_address: u16,
    pub transpose: i8,
    pub initial_attenuation: u8,
}

pub(crate) fn scan(
    bytes: &[u8],
    system: System,
    songs: &mut Vec<SegaPsgSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, system, budget)? else {
        return Ok(());
    };
    scan_profile(bytes, profile, songs, budget, remaining)
}

fn scan_profile(
    bytes: &[u8],
    profile: &Profile,
    songs: &mut Vec<SegaPsgSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    let mut retained = 0;
    let mut unresolved = false;
    for index in 0..usize::from(profile.song_count) {
        budget.charge()?;
        let raw_index = raw_index(index).ok_or(ScanStop::ValidationLimit)?;
        if profile.rejected_selector(raw_index).is_some() {
            unresolved = true;
            continue;
        }
        let song = inspect(bytes, profile, index).ok_or(ScanStop::ValidationLimit)?;
        if stop_only(bytes, &song, profile) {
            continue;
        }
        if retained >= remaining {
            return Err(ScanStop::CandidateLimit);
        }
        songs.push(song);
        retained += 1;
    }
    if unresolved {
        Err(ScanStop::ValidationLimit)
    } else {
        Ok(())
    }
}

pub fn prepare_rom(
    bytes: &[u8],
    song: &SegaPsgSong,
    cancel: &AtomicBool,
) -> AnyResult<PreparedSegaPsg> {
    let mut budget = Budget {
        cancel,
        remaining: 1_000_000,
    };
    let profile = recognized(bytes, song.system, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Sega PSG validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Sega PSG source no longer matches its native profile"))?;
    prepare_profile(bytes, song, profile, cancel)
}

pub fn source_span_matches(media: &MediaIdentity, span: SourceSpan) -> bool {
    let Some(profile) = media
        .sha256
        .as_deref()
        .and_then(|hash| profile_by_identity(media.system, media.byte_len, hash))
    else {
        return false;
    };
    let Some(address) = span.canonical_cpu_address.map(u64::from) else {
        return false;
    };
    let len = u64::from(span.byte_len);
    let offset = u64::from(span.effective_offset);
    let matches = |base: u64, source: u64, size: u64| {
        len != 0
            && address >= base
            && address + len <= base + size
            && offset == source + address - base
            && offset + len <= media.byte_len
    };
    matches(
        u64::from(profile.driver_address),
        profile.audio_offset as u64,
        profile.audio_len() as u64,
    ) || profile.supplemental_selectors.iter().any(|selector| {
        selector.spans.iter().any(|extra| {
            matches(
                u64::from(extra.canonical_cpu_address),
                u64::from(extra.effective_offset),
                u64::from(extra.byte_len),
            )
        })
    })
}

fn prepare_profile(
    bytes: &[u8],
    song: &SegaPsgSong,
    profile: &Profile,
    cancel: &AtomicBool,
) -> AnyResult<PreparedSegaPsg> {
    ensure!(
        inspect(bytes, profile, usize::from(song.index)).as_ref() == Some(song),
        "Sega PSG selection no longer matches its source"
    );
    ensure!(
        !stop_only(bytes, song, profile),
        "Sega PSG selector contains no sequence content"
    );
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Sega PSG preparation cancelled"
    );
    bootstrap::prepare(bytes, profile, song.raw_index)
}

fn recognized(
    bytes: &[u8],
    system: System,
    budget: &mut Budget<'_>,
) -> Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    let candidate =
        all_profiles().any(|profile| profile.system == system && profile.rom_len == bytes.len());
    #[cfg(any(test, feature = "test-support"))]
    let candidate = candidate
        || [
            &fixture::PROFILE,
            &fixture::SIX_BYTE_PROFILE,
            &fixture::SUPPLEMENTAL_PROFILE,
        ]
        .into_iter()
        .any(|profile| system == profile.system && bytes.len() == profile.rom_len);
    if !candidate {
        return Ok(None);
    }
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    // Table shape alone never admits native execution; profiles bind the original image.
    let hash = const_hex::encode(zeff_firmware::sha256_bytes(bytes));
    budget.charge()?;
    Ok(profile_by_identity(
        system.code(),
        bytes.len() as u64,
        &hash,
    ))
}

fn profile_by_identity(system: &str, byte_len: u64, hash: &str) -> Option<&'static Profile> {
    let profile = all_profiles().find(|profile| {
        profile.system.code() == system
            && profile.rom_len as u64 == byte_len
            && profile.sha256 == hash
    });
    #[cfg(any(test, feature = "test-support"))]
    let profile = profile.or_else(|| {
        [
            &fixture::PROFILE,
            &fixture::SIX_BYTE_PROFILE,
            &fixture::SUPPLEMENTAL_PROFILE,
        ]
        .into_iter()
        .find(|profile| {
            system == profile.system.code()
                && byte_len == profile.rom_len as u64
                && hash == profile.sha256
        })
    });
    profile
}

fn inspect(bytes: &[u8], profile: &Profile, index: usize) -> Option<SegaPsgSong> {
    if index >= usize::from(profile.song_count) {
        return None;
    }
    let raw_index = raw_index(index)?;
    if profile.rejected_selector(raw_index).is_some() {
        return None;
    }
    let table_address = profile.table.checked_add(u16::try_from(index * 2).ok()?)?;
    let table_entry = mapped(bytes, profile, table_address, 2)?;
    let header_address = word(bytes, table_entry.effective_offset as usize)?;
    let header_prefix = mapped(bytes, profile, header_address, 6)?;
    let offset = header_prefix.effective_offset as usize;
    let (count_offset, stride) = match profile.header_layout {
        HeaderLayout::FourByteChannels => (2, 4),
        HeaderLayout::SixByteChannels if bytes[offset + 2] == 0 => (3, 6),
        HeaderLayout::SixByteChannels => return None,
    };
    let count = *bytes.get(offset + count_offset)?;
    if !(1..=4).contains(&count) || bytes[offset + 4] == 0 {
        return None;
    }
    let header = mapped(
        bytes,
        profile,
        header_address,
        6 + usize::from(count) * stride,
    )?;
    let mut channels = Vec::with_capacity(usize::from(count));
    for number in 0..count {
        let address = header_address.checked_add(6 + u16::from(number) * stride as u16)?;
        let entry = mapped(bytes, profile, address, stride)?;
        let at = entry.effective_offset as usize;
        let cpu_address = word(bytes, at)?;
        channels.push(SegaPsgChannel {
            number,
            entry,
            sequence: mapped(bytes, profile, cpu_address, 1)?,
            cpu_address,
            transpose: bytes[at + 2] as i8,
            initial_attenuation: bytes[at + 3],
        });
    }
    let bank = mapped(bytes, profile, profile.driver_address, profile.audio_len())?;
    let supplemental = profile.supplemental_spans(raw_index);
    let mut mapped_spans = vec![bank];
    mapped_spans.extend_from_slice(supplemental);
    mapped_spans.sort_by_key(|span| (span.effective_offset, span.canonical_cpu_address));
    let mut warnings = warnings(bytes, profile);
    if !supplemental.is_empty() {
        warnings
            .push("The original envelope lookup also reads the listed low-ROM data ranges.".into());
    }
    Some(SegaPsgSong {
        profile: profile.name,
        system: profile.system,
        region: profile.region,
        timing: SegaPsgTiming::Ntsc,
        frame_divider: profile.frame_divider,
        index: index as u16,
        raw_index,
        title: format!("PSG song 0x{raw_index:02X}"),
        table_entry,
        header,
        driver: mapped(bytes, profile, profile.driver_address, 3)?,
        channels,
        mapped_spans,
        warnings,
    })
}

fn mapped(bytes: &[u8], profile: &Profile, address: u16, len: usize) -> Option<RomSpan> {
    let address = usize::from(address);
    let start = usize::from(profile.driver_address);
    if address < start || address.checked_add(len)? > start.checked_add(profile.audio_len())? {
        return None;
    }
    let offset = profile.audio_offset.checked_add(address - start)?;
    bytes.get(offset..offset.checked_add(len)?)?;
    Some(RomSpan {
        effective_offset: u32::try_from(offset).ok()?,
        byte_len: u32::try_from(len).ok()?,
        canonical_cpu_address: address as u32,
    })
}

fn word(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn raw_index(index: usize) -> Option<u8> {
    0x81_u8.checked_add(u8::try_from(index).ok()?)
}

fn stop_only(bytes: &[u8], song: &SegaPsgSong, profile: &Profile) -> bool {
    song.channels.iter().all(|channel| {
        let mut at = channel.sequence.effective_offset as usize;
        let end = profile.audio_offset + profile.audio_len();
        for _ in 0..=8 {
            let width = match bytes.get(at) {
                Some(0xf2) if at < end => return true,
                // These native six-byte-layout commands only initialize channel RAM.
                Some(0xe0) if profile.header_layout == HeaderLayout::SixByteChannels => 6,
                Some(0xf0) if profile.header_layout == HeaderLayout::SixByteChannels => 5,
                _ => return false,
            };
            if at + width > end || bytes.get(at..at + width).is_none() {
                return false;
            }
            at += width;
        }
        false
    })
}

fn warnings(bytes: &[u8], profile: &Profile) -> Vec<String> {
    let mut warnings = vec![
        "Original PSG driver playback uses the qualified NTSC timing mode; PAL playback is unavailable."
            .into(),
    ];
    let header = [0x7ff0, 0x3ff0, 0x1ff0].into_iter().find_map(|at| {
        (bytes.get(at..at + 8) == Some(b"TMR SEGA".as_slice()))
            .then(|| bytes.get(at + 15).copied())
            .flatten()
    });
    let system = header.and_then(|value| match value >> 4 {
        3 | 4 => Some(System::Sms),
        5..=7 => Some(System::Gg),
        _ => None,
    });
    match system {
        Some(system) if system != profile.system => warnings.push(format!(
            "The source hardware header identifies {}; this exact source is qualified only with {} selected.",
            system.code(), profile.system.code()
        )),
        None => warnings.push(format!(
            "The source has no recognized Sega hardware header; playback uses the exact qualified {} profile.",
            profile.system.code()
        )),
        _ => {}
    }
    for rejected in profile.rejected_selectors {
        warnings.push(format!(
            "Partial native profile: selector 0x{:02X} is omitted. {}",
            rejected.raw_index, rejected.reason
        ));
    }
    warnings
}

fn serialize_system<S: serde::Serializer>(
    system: &System,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(system.code())
}

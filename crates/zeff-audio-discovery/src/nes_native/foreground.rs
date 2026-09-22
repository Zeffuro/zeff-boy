use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    NesNativeChannel, NesNativeProfile, NesNativeSong, NesNativeTiming, PreparedNesNative,
};
use crate::{Budget, MediaIdentity, ScanStop, SourceSpan};

#[cfg(any(test, feature = "test-support"))]
mod fixture;
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use fixture::fixture_rom;

const PROFILE: &str = "nes-native-nrom-foreground-01";
const SOURCE_HASH: &str = "a2df0fc3948a91ef637f804fb793f92ca0a3e510c6ae72d9f9c4738b6194aa5a";
const SOURCE_LEN: usize = 0xa010;
const PATCH: u16 = 0xfd85;
// The original simple NMI path uses these OAM bytes only for sprite DMA.
const READY: u16 = 0x07f0;
const ACK: u16 = 0x07f1;

pub(super) fn owns(profile: &str) -> bool {
    profile == PROFILE || fixture_profile(profile)
}

fn fixture_profile(profile: &str) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    if profile == fixture::PROFILE {
        return true;
    }
    let _ = profile;
    false
}

fn identity(hash: &str, len: usize) -> Option<&'static str> {
    if len != SOURCE_LEN {
        return None;
    }
    if hash == SOURCE_HASH {
        return Some(PROFILE);
    }
    #[cfg(any(test, feature = "test-support"))]
    if hash == fixture::SOURCE_HASH {
        return Some(fixture::PROFILE);
    }
    None
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<&'static str>, ScanStop> {
    if bytes.len() != SOURCE_LEN || bytes.get(..16) != Some(b"NES\x1a\x02\x01\0\0\0\0\0\0\0\0\0\0")
    {
        return Ok(None);
    }
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    Ok(identity(&zeff_firmware::sha256_hex(bytes), bytes.len()))
}

fn streams(profile: &str) -> &'static [&'static [(u16, u16)]] {
    #[cfg(any(test, feature = "test-support"))]
    if profile == fixture::PROFILE {
        return &[&[(0xa500, 0xa501)], &[(0xa510, 0xa511)]];
    }
    let _ = profile;
    STREAMS
}

pub(super) fn scan(
    bytes: &[u8],
    songs: &mut Vec<NesNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(());
    };
    for index in 0..streams(profile).len() {
        budget.charge()?;
        if songs.len() >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        songs.push(inspect(bytes, profile, index).ok_or(ScanStop::ValidationLimit)?);
    }
    Ok(())
}

fn inspect(bytes: &[u8], profile: &'static str, index: usize) -> Option<NesNativeSong> {
    let ranges = streams(profile).get(index)?;
    let table_entry = super::prg_span(bytes, 0xa409 + index as u16 * 2, 2)?;
    let at = table_entry.effective_offset as usize;
    let record = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let first = super::prg_span(bytes, record, 1)?;
    let triple = bytes[first.effective_offset as usize] & 0x80 != 0;
    if ranges.len() != if triple { 3 } else { 1 } {
        return None;
    }
    let header = super::prg_span(bytes, record, if triple { 8 } else { 3 })?;
    let mut channels = Vec::new();
    for (number, &(start, end)) in ranges.iter().enumerate() {
        let relative = if triple { 2 + number * 2 } else { 1 };
        let entry = super::prg_span(bytes, record.checked_add(relative as u16)?, 2)?;
        let at = entry.effective_offset as usize;
        if u16::from_le_bytes([bytes[at], bytes[at + 1]]) != start {
            return None;
        }
        channels.push(NesNativeChannel {
            number: number as u8 + 1,
            entry,
            sequence: super::prg_span(bytes, start, usize::from(end.checked_sub(start)?))?,
        });
    }
    Some(NesNativeSong {
        profile,
        index: index as u16,
        raw_index: index as u8 + 1,
        title: format!("Native audio selector {:02X}", index + 1),
        header,
        table_entry,
        channels,
        native: NesNativeProfile {
            mapper: 0,
            timing: NesNativeTiming::Ntsc,
            init: super::prg_span(bytes, 0x9bf9, 1)?,
            tick: super::prg_span(bytes, 0x9c1a, 1)?,
            driver: super::prg_span(bytes, 0x9bf0, 0xa377 - 0x9bf0)?,
            tables: super::prg_span(bytes, 0xa377, 0xa9ed - 0xa377)?,
            bootstrap: super::prg_span(bytes, PATCH, 19)?,
        },
        mapped_spans: vec![
            super::prg_span(bytes, 0x8000, usize::from(PATCH - 0x8000))?,
            super::prg_span(bytes, PATCH + 19, 0x10000 - usize::from(PATCH + 19))?,
        ],
        warnings: vec![
            "Preserves original cold startup and sound interrupts; replaces the foreground dispatcher after initialization.".into(),
            "Only the listed positive selectors are qualified. Music/effect roles, aliases, natural durations and complete soundtrack coverage remain unknown.".into(),
            "Stream spans retain bounded source reads; native commands are not projected to MIDI or instruments.".into(),
        ],
    })
}

pub(super) fn prepare(
    bytes: &[u8],
    song: &NesNativeSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedNesNative> {
    let mut budget = Budget {
        cancel,
        remaining: 1_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("NES native validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("NES source no longer matches its native profile"))?;
    anyhow::ensure!(
        inspect(bytes, profile, usize::from(song.index)).as_ref() == Some(song),
        "NES native selection no longer matches its source"
    );
    anyhow::ensure!(
        !cancel.load(Ordering::Relaxed),
        "NES native preparation cancelled"
    );
    let code = [
        0xa9,
        1,
        0x8d,
        READY as u8,
        (READY >> 8) as u8,
        0xad,
        ACK as u8,
        (ACK >> 8) as u8,
        0xc9,
        1,
        0xd0,
        0xf9,
        0xa9,
        song.raw_index,
        0x85,
        0xd0,
        0x4c,
        0x95,
        0xfd,
    ];
    let mut prepared = bytes.to_vec();
    let at = usize::from(PATCH - 0x8000) + 16;
    prepared[at..at + code.len()].copy_from_slice(&code);
    Ok(PreparedNesNative {
        bytes: prepared,
        mapper: 0,
        timing: NesNativeTiming::Ntsc,
        ready_address: READY,
        ack_address: ACK,
        wait_start: PATCH + 5,
        wait_end: PATCH + 12,
    })
}

pub(super) fn source_span_matches(media: &MediaIdentity, span: SourceSpan) -> bool {
    media.system == "nes"
        && media.byte_len == SOURCE_LEN as u64
        && media
            .sha256
            .as_deref()
            .is_some_and(|hash| identity(hash, SOURCE_LEN).is_some())
        && span.byte_len != 0
        && span.effective_offset >= 16
        && u64::from(span.effective_offset) + u64::from(span.byte_len) <= 0x8010
        && span.canonical_cpu_address == Some(0x8000 + span.effective_offset - 16)
}

const STREAMS: &[&[(u16, u16)]] = &[
    &[(0xa847, 0xa86b)],
    &[(0xa86b, 0xa87f)],
    &[(0xa87f, 0xa8af)],
    &[(0xa8c0, 0xa8fc)],
    &[(0xa8af, 0xa8c0)],
    &[(0xa8fc, 0xa90d)],
    &[(0xa92b, 0xa953)],
    &[(0xa90d, 0xa92b)],
    &[(0xa4a6, 0xa4c9), (0xa459, 0xa482), (0xa482, 0xa4a6)],
    &[(0xa56d, 0xa584), (0xa4c9, 0xa515), (0xa515, 0xa56d)],
    &[(0xa5dc, 0xa5fc), (0xa584, 0xa5a8), (0xa5a8, 0xa5dc)],
    &[(0xa670, 0xa792), (0xa5fc, 0xa670), (0xa792, 0xa7ff)],
    &[(0xa953, 0xa97e)],
    &[(0xa97e, 0xa9ae)],
    &[(0xa9cb, 0xa9e1)],
    &[(0xa9ae, 0xa9cb)],
];

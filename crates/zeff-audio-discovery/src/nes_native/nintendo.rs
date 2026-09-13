use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    NesNativeChannel, NesNativeProfile, NesNativeSong, NesNativeTiming, PreparedNesNative,
};
use crate::{Budget, MediaIdentity, RomSpan, ScanStop, SourceSpan};

mod bootstrap;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use fixture::fixture_rom;

const SOURCE_LEN: usize = 0x18010;
const SOURCE_HASH: &str = "e5cdad7220096bbc866c32a804aa2421cabd81bf9159642518c08ebd90aac484";
const PROFILE: &str = "nes-native-nintendo-mmc1-01";
const SELECTORS: &[u8] = &[1, 2, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14];

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

fn identity(hash: &str, len: usize) -> Option<(&'static str, &'static [u8])> {
    if len != SOURCE_LEN {
        return None;
    }
    if hash == SOURCE_HASH {
        return Some((PROFILE, SELECTORS));
    }
    #[cfg(any(test, feature = "test-support"))]
    if hash == fixture::SOURCE_HASH {
        return Some((fixture::PROFILE, fixture::SELECTORS));
    }
    None
}

fn recognized(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Option<(&'static str, &'static [u8])>, ScanStop> {
    if bytes.len() != SOURCE_LEN || bytes.get(..8) != Some(b"NES\x1a\x04\x04\x11\x00") {
        return Ok(None);
    }
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    Ok(identity(&zeff_firmware::sha256_hex(bytes), bytes.len()))
}

pub(super) fn scan(
    bytes: &[u8],
    songs: &mut Vec<NesNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    let Some((profile, selectors)) = recognized(bytes, budget)? else {
        return Ok(());
    };
    for (index, &raw) in selectors.iter().enumerate() {
        budget.charge()?;
        if songs.len() >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        for _ in 0..512 {
            budget.charge()?;
        }
        songs.push(inspect(bytes, profile, index, raw).ok_or(ScanStop::ValidationLimit)?);
    }
    Ok(())
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
    let (profile, selectors) = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("NES native validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("NES source no longer matches its native profile"))?;
    let index = usize::from(song.index);
    let inspected = selectors
        .get(index)
        .and_then(|&raw| inspect(bytes, profile, index, raw));
    anyhow::ensure!(
        inspected.as_ref() == Some(song),
        "NES native selection no longer matches its source"
    );
    anyhow::ensure!(
        !cancel.load(Ordering::Relaxed),
        "NES native preparation cancelled"
    );
    bootstrap::build(bytes, song)
}

fn span(bytes: &[u8], address: u16, len: usize) -> Option<RomSpan> {
    super::prg_span(bytes, address, len)
}

fn channel_table(bytes: &[u8], start: u16, finite_entries: Option<usize>) -> Option<RomSpan> {
    if !(0xe30e..0xfd00).contains(&start) {
        return None;
    }
    let limit = finite_entries.unwrap_or(128).checked_mul(2)?;
    if limit == 0 || limit > 256 {
        return None;
    }
    for offset in (0..limit as u16).step_by(2) {
        let row = span(bytes, start.checked_add(offset)?, 2)?;
        let pos = row.effective_offset as usize;
        let target = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
        match target >> 8 {
            0 => return span(bytes, start, usize::from(offset) + 2),
            0xff => {
                let extra = span(bytes, start.checked_add(offset + 2)?, 2)?;
                let pos = extra.effective_offset as usize;
                let next = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
                if !(0xe30e..0xfd00).contains(&next) {
                    return None;
                }
                return span(bytes, start, usize::from(offset) + 4);
            }
            _ if !(0xe30e..0xfd00).contains(&target) => return None,
            _ => {}
        }
    }
    finite_entries.and_then(|_| span(bytes, start, limit))
}

fn inspect(bytes: &[u8], profile: &'static str, index: usize, raw: u8) -> Option<NesNativeSong> {
    let table_entry = span(bytes, 0xe274 + u16::from(raw.checked_sub(1)?), 1)?;
    let offset = bytes[table_entry.effective_offset as usize];
    if !offset.is_multiple_of(10) || offset > 130 {
        return None;
    }
    let header = span(bytes, 0xe282 + u16::from(offset), 10)?;
    let mut channels = Vec::new();
    for number in 0..4 {
        let entry = span(
            bytes,
            header.canonical_cpu_address as u16 + 2 + number * 2,
            2,
        )?;
        let pos = entry.effective_offset as usize;
        let target = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
        if target == 0xffff {
            continue;
        }
        channels.push(NesNativeChannel {
            number: number as u8 + 1,
            entry,
            // Selector 0A stops all channels when its first channel terminates.
            sequence: channel_table(bytes, target, (raw == 10 && number > 0).then_some(2))?,
        });
    }
    if channels.is_empty() {
        return None;
    }
    let native = NesNativeProfile {
        mapper: 1,
        timing: NesNativeTiming::Ntsc,
        init: span(bytes, 0xd2bf, 13)?,
        tick: span(bytes, 0xd470, 0x88)?,
        driver: span(bytes, 0xd26e, 0xdbd)?,
        tables: span(bytes, 0xe02b, 0x1cd5)?,
        bootstrap: span(bytes, 0xff40, 0x90)?,
    };
    let mut mapped_spans = vec![
        span(bytes, 0x8000, 0xa6)?,
        span(bytes, 0x80a9, 0x7e97)?,
        span(bytes, 0xffd0, 0x30)?,
        RomSpan {
            effective_offset: 0xff10,
            byte_len: 0x30,
            canonical_cpu_address: 0xff00,
        },
        RomSpan {
            effective_offset: 0x1000a,
            byte_len: 6,
            canonical_cpu_address: 0xfffa,
        },
    ];
    mapped_spans.sort_unstable();
    Some(NesNativeSong {
        profile,
        index: index as u16,
        raw_index: raw,
        title: format!("Native audio selector {raw:02X}"),
        header,
        table_entry,
        channels,
        native,
        mapped_spans,
        warnings: vec![
            "Only the listed original music and cue selectors are qualified; silence/control selectors 03 and 06 are excluded and full soundtrack coverage is not established.".into(),
            "Original MMC1 startup, NMI handler and frame wait are retained with the active PRG bank; native channel tables and patterns are preserved without MIDI conversion.".into(),
        ],
    })
}

pub(super) fn source_span_matches(media: &MediaIdentity, span: SourceSpan) -> bool {
    if media.system != "nes"
        || media.byte_len != SOURCE_LEN as u64
        || !media
            .sha256
            .as_deref()
            .is_some_and(|hash| identity(hash, media.byte_len as usize).is_some())
        || span.byte_len == 0
    {
        return false;
    }
    let offset = span.effective_offset;
    let end = u64::from(offset) + u64::from(span.byte_len);
    (offset >= 16 && end <= 0x8010 && span.canonical_cpu_address == Some(0x8000 + offset - 16))
        || (offset >= 0xff10 && end <= 0xff40 && span.canonical_cpu_address == Some(offset - 16))
        || (offset >= 0x1000a && end <= 0x10010 && span.canonical_cpu_address == Some(offset - 16))
}

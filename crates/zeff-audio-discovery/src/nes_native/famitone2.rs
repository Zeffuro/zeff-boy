use std::sync::atomic::{AtomicBool, Ordering};

use super::{NesNativeProfile, NesNativeSong, NesNativeTiming, PreparedNesNative, prg_span};
use crate::{Budget, MediaIdentity, ScanStop, SourceSpan};

mod bootstrap;
mod data;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
mod recognition;
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub(super) use fixture::{four_channel_rom, rom};

const PROFILE: &str = "famitone2-v111-ntsc-pulse-pair";
const FOUR_CHANNEL_PROFILE: &str = "famitone2-v111-ntsc-four-channel";

pub(crate) fn owns(profile: &str) -> bool {
    matches!(profile, PROFILE | FOUR_CHANNEL_PROFILE)
}

fn inspect(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<NesNativeSong>, ScanStop> {
    let Some(layout) = recognition::inspect(bytes, budget)? else {
        return Ok(Vec::new());
    };
    let Some(data) = data::inspect(bytes, layout.header, budget)? else {
        return Ok(Vec::new());
    };
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    let source_sha256 = zeff_firmware::sha256_hex(bytes);
    let driver = prg_span(bytes, layout.init, usize::from(recognition::ENGINE_LEN)).unwrap();
    let native = NesNativeProfile {
        mapper: 0,
        timing: NesNativeTiming::Ntsc,
        init: prg_span(bytes, layout.init, 0x55).unwrap(),
        tick: prg_span(bytes, layout.init + 0x11f, 0x169).unwrap(),
        driver,
        tables: data.header,
        bootstrap: prg_span(bytes, recognition::BOOTSTRAP, 256).unwrap(),
    };
    let mut mapped = data.spans;
    mapped.push(driver);
    mapped.push(prg_span(bytes, layout.reset, 0x56).unwrap());
    mapped.sort_unstable();
    mapped.dedup();
    Ok((0..2).map(|index| NesNativeSong {
        profile: if data.four_channels { FOUR_CHANNEL_PROFILE } else { PROFILE },
        source_sha256: Some(source_sha256.clone()),
        index: index as u16,
        raw_index: index as u8,
        title: format!("FamiTone2 cue {}", index + 1),
        header: data.header,
        table_entry: data.entries[index],
        channels: data.channels[index].clone(),
        native: native.clone(),
        mapped_spans: mapped.clone(),
        warnings: vec!["Isolated cold-state playback of the declared cue, updated by NTSC NMI; game startup timing and complete soundtrack coverage are not established.".into()],
    }).collect())
}

pub(super) fn scan(
    bytes: &[u8],
    songs: &mut Vec<NesNativeSong>,
    budget: &mut Budget<'_>,
    capacity: usize,
) -> Result<(), ScanStop> {
    for song in inspect(bytes, budget)? {
        budget.charge()?;
        if songs.len() >= capacity {
            return Err(ScanStop::CandidateLimit);
        }
        songs.push(song);
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
        remaining: 100_000,
    };
    let songs = inspect(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("FamiTone2 validation stopped: {stop:?}"))?;
    anyhow::ensure!(
        songs.get(usize::from(song.index)) == Some(song),
        "FamiTone2 selection no longer matches its source"
    );
    anyhow::ensure!(
        !cancel.load(Ordering::Relaxed),
        "FamiTone2 preparation cancelled"
    );
    bootstrap::build(bytes, song)
}

pub(super) fn source_span_matches(bytes: &[u8], media: &MediaIdentity, span: SourceSpan) -> bool {
    if media.system != "nes" || media.byte_len != bytes.len() as u64 || span.byte_len == 0 {
        return false;
    }
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let Ok(songs) = inspect(bytes, &mut budget) else {
        return false;
    };
    songs
        .first()
        .is_some_and(|song| retained_span_matches(song, media, span))
}

pub(super) fn retained_span_matches(
    song: &NesNativeSong,
    media: &MediaIdentity,
    span: SourceSpan,
) -> bool {
    owns(song.profile)
        && song.source_sha256.is_some()
        && song.source_sha256 == media.sha256
        && media.system == "nes"
        && media.byte_len == 0xa010
        && song.native.mapper == 0
        && song.native.timing == NesNativeTiming::Ntsc
        && span.byte_len != 0
        && song.mapped_spans.len() <= 258
        && song.mapped_spans.iter().any(|mapped| {
            if mapped.effective_offset < 16
                || u64::from(mapped.effective_offset) + u64::from(mapped.byte_len) > 0x7810
                || mapped.canonical_cpu_address != 0x8000 + mapped.effective_offset - 16
            {
                return false;
            }
            let delta = span.effective_offset.checked_sub(mapped.effective_offset);
            delta.is_some_and(|delta| {
                delta
                    .checked_add(span.byte_len)
                    .is_some_and(|end| end <= mapped.byte_len)
                    && span.canonical_cpu_address == Some(mapped.canonical_cpu_address + delta)
            })
        })
}

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result as AnyResult, ensure};
use serde::Serialize;

use super::{Budget, MAX_ROM_BYTES, RomSpan, ScanStop, word};

mod bootstrap;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
mod profiles;
#[cfg(test)]
mod tests;

pub use bootstrap::PreparedAasPcmRom;
#[cfg(any(test, feature = "test-support"))]
pub use fixture::fixture_rom;
use profiles::Profile;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AasPcmSong {
    pub root: RomSpan,
    pub header: RomSpan,
    pub index: u16,
    pub title: String,
    pub channels: u8,
    pub sample_rate: u32,
    pub sample_data: RomSpan,
    pub sample_leadin: RomSpan,
    pub sample_padding: RomSpan,
    pub loop_start: Option<u32>,
    pub native: AasPcmNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AasPcmNativeProfile {
    pub profile: &'static str,
    pub handoff: RomSpan,
    pub init: RomSpan,
    pub play: RomSpan,
    pub update: RomSpan,
    pub timer1_irq: RomSpan,
    pub irq_handler: RomSpan,
    pub irq_handler_address: u32,
    pub vblank_slot_address: u32,
    pub sample_bank_address: u32,
    pub selector_binding: RomSpan,
    pub playback_channel: u8,
    pub volume: u8,
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<AasPcmSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(());
    };
    for index in 0..profile.count {
        budget.charge()?;
        if usize::from(index) >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        songs.push(inspect(bytes, profile, index).ok_or(ScanStop::ValidationLimit)?);
    }
    Ok(())
}

pub fn prepare_rom(
    bytes: &[u8],
    song: &AasPcmSong,
    cancel: &AtomicBool,
) -> AnyResult<PreparedAasPcmRom> {
    let mut budget = Budget {
        cancel,
        remaining: 1_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("AAS PCM validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("AAS PCM source no longer matches its native profile"))?;
    ensure!(
        inspect(bytes, profile, song.index).as_ref() == Some(song),
        "AAS PCM selection no longer matches its source"
    );
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "AAS PCM preparation cancelled"
    );
    bootstrap::build(bytes, song)
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    let matching = |profile: &&Profile| {
        bytes.len() == profile.rom_len && bytes.get(0xa0..0xb0) == Some(profile.header)
    };
    if !profiles::all().any(|profile| matching(&profile)) {
        return Ok(None);
    }
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    let sha256 = zeff_firmware::sha256_hex(bytes);
    Ok(profiles::all()
        .filter(matching)
        .find(|profile| profile.sha256 == sha256))
}

fn inspect(bytes: &[u8], profile: &Profile, index: u16) -> Option<AasPcmSong> {
    if bytes.len() != profile.rom_len || index >= profile.count {
        return None;
    }
    let span = |offset: usize, len: usize| {
        (len != 0 && offset.checked_add(len)? <= bytes.len()).then(|| RomSpan::new(offset, len))
    };
    let part = |(offset, len)| span(offset, len);
    let root = span(profile.table, usize::from(profile.count) * 12)?;
    let header = span(profile.table + usize::from(index) * 12, 12)?;
    let at = header.effective_offset as usize;
    let start = word(bytes, at)? as usize;
    let end = word(bytes, at + 4)? as usize;
    let repeat = word(bytes, at + 8)? as usize;
    if start < 16 || end <= start || (repeat != 0 && !(start..end).contains(&repeat)) {
        return None;
    }
    let sample_data = span(profile.bank.checked_add(start)?, end - start)?;
    // DMA alignment and loop block wrapping consume the source's surrounding guard bytes.
    let sample_leadin = span(profile.bank.checked_add(start)?.checked_sub(16)?, 16)?;
    let sample_padding = span(profile.bank.checked_add(end)?, 16)?;
    let native = AasPcmNativeProfile {
        profile: profile.id,
        handoff: part(profile.handoff)?,
        init: part(profile.init)?,
        play: part(profile.play)?,
        update: part(profile.update)?,
        timer1_irq: part(profile.timer1_irq)?,
        irq_handler: part(profile.irq_handler)?,
        irq_handler_address: profile.irq_address,
        vblank_slot_address: profile.vblank_slot,
        sample_bank_address: 0x0800_0000 + profile.bank as u32,
        selector_binding: part(profile.binding)?,
        playback_channel: if (profile.channel_zero.0..=profile.channel_zero.1).contains(&index) {
            0
        } else {
            3
        },
        volume: 64,
    };
    let mut mapped_spans = vec![
        root,
        sample_data,
        sample_leadin,
        sample_padding,
        native.init,
        native.irq_handler,
        native.selector_binding,
    ];
    for &range in profile.support {
        mapped_spans.push(part(range)?);
    }
    mapped_spans.sort_unstable_by_key(|span| (span.effective_offset, span.byte_len));
    mapped_spans.dedup();
    if mapped_spans
        .iter()
        .any(|&span| intersects(span, native.handoff))
    {
        return None;
    }
    Some(AasPcmSong {
        root,
        header,
        index,
        title: format!("AAS PCM cue {index:03}"),
        channels: 1,
        sample_rate: 10_400,
        sample_data,
        sample_leadin,
        sample_padding,
        loop_start: (repeat != 0).then(|| (repeat - start) as u32),
        native,
        mapped_spans,
        warnings: vec!["Original signed 8-bit PCM cue playback at full volume, using the source selector's isolated channel. Game context and option volumes can differ; cues include effects and speech.".into()],
    })
}

fn intersects(left: RomSpan, right: RomSpan) -> bool {
    left.effective_offset < right.effective_offset + right.byte_len
        && right.effective_offset < left.effective_offset + left.byte_len
}

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

pub use bootstrap::PreparedAasStreamRom;
#[cfg(any(test, feature = "test-support"))]
pub use fixture::fixture_rom;
use profiles::Profile;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AasStreamSong {
    pub root: RomSpan,
    pub header: RomSpan,
    pub index: u16,
    pub title: String,
    pub channels: u8,
    pub encoded_nibbles: u32,
    pub encoded_data: RomSpan,
    pub decoder_lookahead: RomSpan,
    pub loops: bool,
    pub native: AasStreamNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AasStreamNativeProfile {
    pub profile: &'static str,
    pub handoff: RomSpan,
    pub init: RomSpan,
    pub play: RomSpan,
    pub update: RomSpan,
    pub timer1_irq: RomSpan,
    pub decoder: RomSpan,
    pub irq_handler: RomSpan,
    pub irq_handler_address: u32,
    pub vblank_slot_address: u32,
    pub stream_bank_address: u32,
    pub selector_binding: RomSpan,
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<AasStreamSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(());
    };
    for ordinal in 0..profile.count {
        budget.charge()?;
        if usize::from(ordinal) >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        let index = profile
            .first
            .checked_add(ordinal)
            .ok_or(ScanStop::ValidationLimit)?;
        songs.push(inspect(bytes, profile, index).ok_or(ScanStop::ValidationLimit)?);
    }
    Ok(())
}

pub fn prepare_rom(
    bytes: &[u8],
    song: &AasStreamSong,
    cancel: &AtomicBool,
) -> AnyResult<PreparedAasStreamRom> {
    let mut budget = Budget {
        cancel,
        remaining: 1_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("AAS stream validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("AAS stream source no longer matches its native profile"))?;
    ensure!(
        inspect(bytes, profile, song.index).as_ref() == Some(song),
        "AAS stream selection no longer matches its source"
    );
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "AAS stream preparation cancelled"
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

fn inspect(bytes: &[u8], profile: &Profile, index: u16) -> Option<AasStreamSong> {
    if bytes.len() != profile.rom_len || index.checked_sub(profile.first)? >= profile.count {
        return None;
    }
    let span = |offset: usize, len: usize| {
        (len != 0 && offset.checked_add(len)? <= bytes.len()).then(|| RomSpan::new(offset, len))
    };
    let part = |(offset, len)| span(offset, len);
    let root = span(
        profile.table + usize::from(profile.first) * 12,
        usize::from(profile.count) * 12,
    )?;
    let header = span(profile.table + usize::from(index) * 12, 12)?;
    let at = header.effective_offset as usize;
    let offset = word(bytes, at)? as usize;
    let encoded_nibbles = word(bytes, at + 4)?;
    if encoded_nibbles == 0 || word(bytes, at + 8)? != 0x10ff {
        return None;
    }
    let encoded_data = span(
        profile.bank.checked_add(offset)?,
        encoded_nibbles.div_ceil(2) as usize,
    )?;
    let decoder_lookahead = span(
        encoded_data.effective_offset as usize + encoded_data.byte_len as usize,
        profile.lookahead,
    )?;
    let native = AasStreamNativeProfile {
        profile: profile.id,
        handoff: part(profile.handoff)?,
        init: part(profile.init)?,
        play: part(profile.play)?,
        update: part(profile.update)?,
        timer1_irq: part(profile.timer1_irq)?,
        decoder: part(profile.decoder)?,
        irq_handler: part(profile.irq_handler)?,
        irq_handler_address: profile.irq_address,
        vblank_slot_address: profile.vblank_slot,
        stream_bank_address: 0x0800_0000 + profile.bank as u32,
        selector_binding: part(profile.binding)?,
    };
    let mut mapped_spans = vec![
        root,
        encoded_data,
        decoder_lookahead,
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
    Some(AasStreamSong {
        root,
        header,
        index,
        title: format!("AAS stream {index:03}"),
        channels: 1,
        encoded_nibbles,
        encoded_data,
        decoder_lookahead,
        loops: true,
        native,
        mapped_spans,
        warnings: vec!["Original compressed-stream playback; mapped bytes include decoder lookahead. Other audio selectors and instrument conversion remain unmapped.".into()],
    })
}

fn intersects(left: RomSpan, right: RomSpan) -> bool {
    left.effective_offset < right.effective_offset + right.byte_len
        && right.effective_offset < left.effective_offset + left.byte_len
}

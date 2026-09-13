use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};

use super::{
    GbNativeChannel, GbNativeProfile, GbNativeSong, GbNativeTiming, PreparedGbNative, rom_span,
};
use crate::{Budget, MediaIdentity, ScanStop};

mod bootstrap;
#[cfg(any(test, feature = "test-support"))]
pub(super) mod fixture;
mod profiles;
#[cfg(test)]
mod tests;

pub(super) struct Cue {
    raw: u8,
    frames: u32,
    clocks: u64,
    loop_start: Option<u32>,
    banks: &'static [u8],
}

pub(super) struct Profile {
    id: &'static str,
    source: &'static str,
    cartridge_type: u8,
    init: u16,
    init_len: u16,
    selector: u16,
    tick: u16,
    driver: u16,
    driver_end: u16,
    table: u16,
    table_rows: u16,
    hook: u16,
    cues: &'static [Cue],
}

pub(super) fn candidate(bytes: &[u8]) -> bool {
    bytes.len() == 0x20_0000
        && bytes.get(0x143) == Some(&0xc0)
        && matches!(bytes.get(0x147..0x14a), Some([0x19, 6, 0] | [0x1b, 6, 2]))
}

fn recognized(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> std::result::Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    if !candidate(bytes) {
        return Ok(None);
    }
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    let source = zeff_firmware::sha256_hex(bytes);
    Ok(profiles::all().find(|profile| profile.source == source))
}

pub(super) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> std::result::Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(());
    };
    for index in 0..profile.cues.len() {
        budget.charge()?;
        if songs.len() >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        let song = inspect(bytes, profile, index).ok_or(ScanStop::ValidationLimit)?;
        for _ in &song.channels {
            budget.charge()?;
        }
        songs.push(song);
    }
    Ok(())
}

pub(super) fn prepare_rom(
    bytes: &[u8],
    song: &GbNativeSong,
    cancel: &AtomicBool,
) -> Result<PreparedGbNative> {
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
    bootstrap::build(bytes, song, profile.selector)
}

fn inspect(bytes: &[u8], profile: &Profile, index: usize) -> Option<GbNativeSong> {
    let cue = profile.cues.get(index)?;
    let row = u16::from(cue.raw).checked_sub(0x30)?;
    if row >= profile.table_rows || cue.frames == 0 || cue.clocks == 0 {
        return None;
    }
    let table_entry = rom_span(bytes, 0, profile.table.checked_add(row * 3)?, 3)?;
    let offset = table_entry.effective_offset as usize;
    let bank = *bytes.get(offset)?;
    let address = u16::from_le_bytes([*bytes.get(offset + 1)?, *bytes.get(offset + 2)?]);
    let header_byte = rom_span(bytes, bank, address, 1)?;
    let mask = *bytes.get(header_byte.effective_offset as usize)?;
    if mask == 0 || mask & !0x0f != 0 || !cue.banks.contains(&bank) {
        return None;
    }
    let header = rom_span(bytes, bank, address, 1 + mask.count_ones() as usize * 2)?;
    let mut channels = Vec::new();
    for number in 0..4 {
        if mask & (1 << number) == 0 {
            continue;
        }
        let entry = rom_span(
            bytes,
            bank,
            address.checked_add(1 + channels.len() as u16 * 2)?,
            2,
        )?;
        let offset = entry.effective_offset as usize;
        let pointer = u16::from_le_bytes([*bytes.get(offset)?, *bytes.get(offset + 1)?]);
        channels.push(GbNativeChannel {
            number: number + 1,
            entry,
            sequence: rom_span(bytes, bank, pointer, 1)?,
        });
    }
    let native = GbNativeProfile {
        cartridge_type: profile.cartridge_type,
        timing: GbNativeTiming::CgbDouble,
        init: rom_span(bytes, 0, profile.init, usize::from(profile.init_len))?,
        tick: rom_span(bytes, 0, profile.tick, 0x2a)?,
        driver: rom_span(
            bytes,
            0,
            profile.driver,
            usize::from(profile.driver_end - profile.driver),
        )?,
        tables: rom_span(bytes, 0, profile.table, usize::from(profile.table_rows) * 3)?,
        bootstrap: rom_span(bytes, 0, 0x3f00, 0x40)?,
        startup_hook: rom_span(bytes, 0, profile.hook, 3)?,
    };
    let irq = u16::from_le_bytes([*bytes.get(0x41)?, *bytes.get(0x42)?]);
    let mut mapped_spans = vec![
        rom_span(bytes, 0, 0, 0x10)?,
        rom_span(bytes, 0, 0x40, 3)?,
        rom_span(bytes, 0, irq, 0x80)?,
        rom_span(
            bytes,
            0,
            profile.driver,
            usize::from(0x3f00 - profile.driver),
        )?,
    ];
    for &bank in cue.banks {
        mapped_spans.push(rom_span(bytes, bank, 0x4000, 0x4000)?);
    }
    Some(GbNativeSong {
        profile: profile.id,
        index: index as u16,
        raw_index: cue.raw,
        title: format!("Native music selector {:02X}", cue.raw),
        bank,
        header,
        table_entry,
        channels,
        native,
        mapped_spans,
        playback_frames: cue.frames,
        playback_clocks: cue.clocks,
        loop_start_frame: cue.loop_start,
        warnings: vec![
            "Only the listed source-bound music selectors are qualified; control selectors, sound effects and unqualified music are not enumerated.".into(),
            "Channel sequence spans identify each initial command byte. The complete referenced audio banks and original driver context are retained for shared banked streams, commands and waveform data; native commands are not projected to MIDI or instrument programs.".into(),
        ],
    })
}

pub(super) fn source_matches(media: &MediaIdentity) -> bool {
    matches!(media.system, "gb" | "gbc")
        && media.byte_len == 0x20_0000
        && media
            .sha256
            .as_deref()
            .is_some_and(|hash| profiles::all().any(|profile| profile.source == hash))
}

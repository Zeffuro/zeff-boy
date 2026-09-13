use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result as AnyResult, ensure};
use serde::Serialize;

use super::{Budget, MAX_ROM_BYTES, RomSpan, ScanStop, word};

mod banked;
mod bootstrap;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
#[cfg(any(test, feature = "test-support"))]
mod module_fixture;
#[cfg(test)]
mod module_tests;
mod modules;
mod profiles;
mod separate;
#[cfg(any(test, feature = "test-support"))]
mod separate_fixture;
#[cfg(test)]
mod separate_tests;
#[cfg(test)]
mod tests;

pub use bootstrap::PreparedGbassRom;
#[cfg(any(test, feature = "test-support"))]
pub use fixture::{
    fixture_rom, fixture_rom_banked, fixture_rom_irq, fixture_rom_partial, fixture_rom_started,
};
#[cfg(any(test, feature = "test-support"))]
pub use module_fixture::fixture_rom_module;
pub use modules::GbassModule;
use profiles::{Profile, all_profiles};
#[cfg(any(test, feature = "test-support"))]
pub use separate_fixture::fixture_rom_separate;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbassSong {
    pub root: RomSpan,
    pub header: RomSpan,
    pub index: u16,
    pub title: String,
    pub channels: u8,
    pub instruments: u16,
    pub samples: u16,
    pub tracks: Vec<GbassTrack>,
    pub native: GbassNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbassTrack {
    pub header: RomSpan,
    pub sequence: RomSpan,
    pub channel: u8,
    pub initial_volume: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GbassSchedule {
    VblankThenMain,
    VblankIrq,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GbassStartupGuard {
    pub address: u32,
    pub byte_len: u8,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GbassBankSelector {
    pub index: u16,
    pub table: RomSpan,
    pub configuration_address: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbassNativeProfile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<GbassModule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank: Option<GbassBankSelector>,
    pub profile: &'static str,
    pub schedule: GbassSchedule,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hardware_started_before_handoff: bool,
    pub startup_guards: Vec<GbassStartupGuard>,
    pub handoff: RomSpan,
    pub init: RomSpan,
    pub hardware_start: RomSpan,
    pub play: RomSpan,
    pub vblank: RomSpan,
    pub update: RomSpan,
    pub update_wrapper: RomSpan,
    pub vblank_wait: RomSpan,
    pub irq_handler: RomSpan,
    pub irq_handler_address: u32,
    pub irq_table: RomSpan,
    pub irq_table_address: u32,
    pub state_address: u32,
    pub song_table: RomSpan,
    pub sample_table: RomSpan,
    pub instrument_table: RomSpan,
    pub instrument_types: RomSpan,
    pub sequence_bank: RomSpan,
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbassSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(());
    };
    let first = songs.len();
    for bank in all_profiles().filter(|bank| bank.sha256 == profile.sha256) {
        scan_profile(
            bytes,
            bank,
            songs,
            budget,
            max_candidates.saturating_sub(songs.len() - first),
        )?;
    }
    Ok(())
}

fn scan_profile(
    bytes: &[u8],
    profile: &Profile,
    songs: &mut Vec<GbassSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    for index in 0..profile.song_count {
        budget.charge()?;
        if usize::from(index) >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        charge_song(profile, budget)?;
        songs.push(inspect(bytes, profile, index).ok_or(ScanStop::ValidationLimit)?);
    }
    if profile.partial_warning.is_some() {
        Err(ScanStop::ValidationLimit)
    } else {
        Ok(())
    }
}

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbassSong,
    cancel: &AtomicBool,
) -> AnyResult<PreparedGbassRom> {
    let mut budget = Budget {
        cancel,
        remaining: 1_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("GBASS validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("GBASS source no longer matches its native profile"))?;
    let profile = all_profiles()
        .find(|candidate| {
            candidate.sha256 == profile.sha256
                && candidate.config == song.root.effective_offset as usize
        })
        .ok_or_else(|| anyhow::anyhow!("GBASS bank no longer matches its source"))?;
    charge_song(profile, &mut budget)
        .map_err(|stop| anyhow::anyhow!("GBASS validation stopped: {stop:?}"))?;
    ensure!(
        inspect(bytes, profile, song.index).as_ref() == Some(song),
        "GBASS selection no longer matches its source"
    );
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "GBASS preparation cancelled"
    );
    bootstrap::build(bytes, song)
}

fn charge_song(profile: &Profile, budget: &mut Budget<'_>) -> Result<(), ScanStop> {
    for _ in 0..usize::from(profile.samples) + usize::from(profile.instruments) + 16 {
        budget.charge()?;
    }
    Ok(())
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    let matching = |profile: &&Profile| {
        bytes.len() == profile.rom_len
            && bytes.get(0xa0..0xb0) == Some(profile.header)
            && word(bytes, profile.config + 4) == Some(u32::from(profile.song_count))
    };
    if !all_profiles().any(|profile| matching(&profile)) {
        return Ok(None);
    }
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    let sha256 = zeff_firmware::sha256_hex(bytes);
    Ok(all_profiles()
        .filter(matching)
        .find(|profile| profile.sha256 == sha256))
}

fn inspect(bytes: &[u8], profile: &Profile, index: u16) -> Option<GbassSong> {
    if index >= profile.song_count || bytes.len() != profile.rom_len {
        return None;
    }
    let config = profile.config;
    let root = span(bytes, config, profile.layout.byte_len())?;
    let shift = profile.layout.table_shift(bytes, config)?;
    modules::validate(bytes, profile, root)?;
    if word(bytes, config + 4)? != u32::from(profile.song_count)
        || word(bytes, config + 12 + shift)? != u32::from(profile.instruments)
        || word(bytes, config + 24 + shift)? != u32::from(profile.samples)
    {
        return None;
    }
    let song_table = pointer_span(
        bytes,
        profile,
        word(bytes, config + 8)?,
        usize::from(profile.song_count) * 12,
    )?;
    let sample_table = pointer_span(
        bytes,
        profile,
        word(bytes, config + 28 + shift)?,
        usize::from(profile.samples) * 24,
    )?;
    let instrument_table = pointer_span(
        bytes,
        profile,
        word(bytes, config + 16 + shift)?,
        usize::from(profile.instruments) * 4,
    )?;
    let instrument_types = pointer_span(
        bytes,
        profile,
        word(bytes, config + 20 + shift)?,
        usize::from(profile.instruments),
    )?;
    let sequence_bank = span(bytes, profile.sequences.0, profile.sequences.1)?;
    let mut mapped = vec![
        root,
        song_table,
        sample_table,
        instrument_table,
        instrument_types,
        sequence_bank,
    ];
    if let Some(bank) = profile.bank {
        let table = span(
            bytes,
            bank.table.effective_offset as usize,
            bank.table.byte_len as usize,
        )?;
        if usize::from(bank.index) * 4 + 4 > table.byte_len as usize
            || word(
                bytes,
                table.effective_offset as usize + usize::from(bank.index) * 4,
            )? != root.canonical_cpu_address
        {
            return None;
        }
        mapped.push(table);
    }
    if let Some(module) = profile.module {
        mapped.push(module.loader);
    }
    map_samples(bytes, profile, sample_table, &mut mapped)?;
    map_instruments(bytes, profile, instrument_table, &mut mapped)?;

    let header = span(
        bytes,
        song_table.effective_offset as usize + usize::from(index) * 12,
        12,
    )?;
    let offset = header.effective_offset as usize;
    let channels = u8::try_from(word(bytes, offset)?).ok()?;
    if !(1..=12).contains(&channels) {
        return None;
    }
    let channel_table = pointer_span(
        bytes,
        profile,
        word(bytes, offset + 4)?,
        usize::from(channels) * 8,
    )?;
    if !contains(
        span(bytes, profile.channel_data.0, profile.channel_data.1)?,
        channel_table,
    ) {
        return None;
    }
    let title_offset =
        pointer_span(bytes, profile, word(bytes, offset + 8)?, 1)?.effective_offset as usize;
    let title_bank = span(bytes, profile.title_data.0, profile.title_data.1)?;
    if !contains(title_bank, span(bytes, title_offset, 1)?) {
        return None;
    }
    let title_bytes = bytes.get(title_offset..title_offset.checked_add(64)?.min(bytes.len()))?;
    let title_len = title_bytes.iter().position(|&byte| byte == 0)?;
    let title = std::str::from_utf8(&title_bytes[..title_len]).ok()?;
    if !title
        .bytes()
        .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        return None;
    }
    let title_span = span(bytes, title_offset, title_len + 1)?;
    if !contains(title_bank, title_span) {
        return None;
    }
    mapped.push(title_span);
    mapped.push(channel_table);
    let mut tracks = Vec::with_capacity(usize::from(channels));
    let mut used_channels = 0_u16;
    for track in 0..usize::from(channels) {
        let entry = channel_table.effective_offset as usize + track * 8;
        let sequence = pointer_span(bytes, profile, word(bytes, entry)?, 1)?;
        if !contains(sequence_bank, sequence) {
            return None;
        }
        let channel = u8::try_from(half(bytes, entry + 4)?).ok()?;
        let initial_volume = half(bytes, entry + 6)?;
        if channel >= 12 || used_channels & (1 << channel) != 0 || initial_volume > 256 {
            return None;
        }
        used_channels |= 1 << channel;
        tracks.push(GbassTrack {
            header: span(bytes, entry, 8)?,
            sequence,
            channel,
            initial_volume,
        });
    }
    let native = profile.native(
        song_table,
        sample_table,
        instrument_table,
        instrument_types,
        sequence_bank,
    );
    for routine in [
        native.init,
        native.hardware_start,
        native.play,
        native.vblank,
        native.update,
        native.update_wrapper,
        native.vblank_wait,
        native.irq_handler,
        native.irq_table,
    ] {
        span(
            bytes,
            routine.effective_offset as usize,
            routine.byte_len as usize,
        )?;
        mapped.push(routine);
    }
    merge_spans(&mut mapped);
    if mapped.iter().any(|&part| {
        intersects(part, native.handoff)
            || native
                .module
                .is_some_and(|module| intersects(part, module.loader_handoff))
    }) {
        return None;
    }
    let mut warnings = vec![
        "Native playback preserves the qualified original startup, sequencer, mixer and VBlank buffer schedule.".into(),
        "Mapped ranges identify original tables, PCM samples, sequence banks and routine witnesses; they are a partial inventory and do not establish soundtrack or loop completeness.".into(),
    ];
    if let Some(warning) = profile.partial_warning {
        warnings.push(warning.into());
    }
    Some(GbassSong {
        root,
        header,
        index,
        title: if title.is_empty() {
            format!("Song {}", index + 1)
        } else {
            title.to_owned()
        },
        channels,
        instruments: profile.instruments,
        samples: profile.samples,
        tracks,
        native,
        mapped_spans: mapped,
        warnings,
    })
}

fn map_samples(
    bytes: &[u8],
    profile: &Profile,
    table: RomSpan,
    mapped: &mut Vec<RomSpan>,
) -> Option<()> {
    let bank = span(bytes, profile.sample_data.0, profile.sample_data.1)?;
    for index in 0..usize::from(profile.samples) {
        let offset = table.effective_offset as usize + index * 24;
        let length = word(bytes, offset + 12)?;
        let loop_start = word(bytes, offset + 16)?;
        let loop_end = word(bytes, offset + 20)?;
        if !profile.sample_steps.contains(&word(bytes, offset + 4)?)
            || word(bytes, offset + 8)? != 0
            || length == 0
            || (length | loop_start | loop_end) & 0xffff != 0
            || loop_start > loop_end
        {
            return None;
        }
        // Original loop endpoints can extend beyond the one-shot endpoint.
        let pointer = word(bytes, offset)?;
        if !profile.sample_flags.contains(&(pointer & 0xf000_0000)) {
            return None;
        }
        // The original decoder separates the sample format flag from its address.
        let data = pointer_span(
            bytes,
            profile,
            pointer & 0x0fff_ffff,
            (length.max(loop_end) >> 16) as usize,
        )?;
        if !contains(bank, data) {
            return None;
        }
        mapped.push(data);
    }
    Some(())
}

fn map_instruments(
    bytes: &[u8],
    profile: &Profile,
    table: RomSpan,
    mapped: &mut Vec<RomSpan>,
) -> Option<()> {
    let bank = span(bytes, profile.instrument_data.0, profile.instrument_data.1)?;
    for index in 0..usize::from(profile.instruments) {
        let pointer = word(bytes, table.effective_offset as usize + index * 4)?;
        if !contains(bank, pointer_span(bytes, profile, pointer, 1)?) {
            return None;
        }
    }
    mapped.push(bank);
    Some(())
}

fn half(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn pointer_span(bytes: &[u8], profile: &Profile, address: u32, len: usize) -> Option<RomSpan> {
    if let Some(module) = profile.module {
        return module.source_span(bytes, address, len);
    }
    let offset = address.checked_sub(0x0800_0000)? as usize;
    span(bytes, offset, len)
}

fn span(bytes: &[u8], offset: usize, len: usize) -> Option<RomSpan> {
    let end = offset.checked_add(len)?;
    (len != 0 && end <= bytes.len() && end <= MAX_ROM_BYTES).then(|| RomSpan::new(offset, len))
}

fn contains(outer: RomSpan, inner: RomSpan) -> bool {
    inner.effective_offset >= outer.effective_offset
        && u64::from(inner.effective_offset) + u64::from(inner.byte_len)
            <= u64::from(outer.effective_offset) + u64::from(outer.byte_len)
}

fn intersects(a: RomSpan, b: RomSpan) -> bool {
    u64::from(a.effective_offset) < u64::from(b.effective_offset) + u64::from(b.byte_len)
        && u64::from(b.effective_offset) < u64::from(a.effective_offset) + u64::from(a.byte_len)
}

fn merge_spans(spans: &mut Vec<RomSpan>) {
    spans.sort_unstable();
    let mut merged: Vec<RomSpan> = Vec::with_capacity(spans.len());
    for part in spans.drain(..) {
        if let Some(last) = merged.last_mut()
            && part.effective_offset <= last.effective_offset + last.byte_len
        {
            last.byte_len = last
                .byte_len
                .max(part.effective_offset + part.byte_len - last.effective_offset);
        } else {
            merged.push(part);
        }
    }
    *spans = merged;
}

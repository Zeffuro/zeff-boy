use std::mem::size_of;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, ensure};
use serde::Serialize;

use super::{Budget, MAX_ROM_BYTES, RomSpan, ScanStop, rom_pointer, word};

mod driver;
mod placement;
mod signatures;
mod songs;
mod startup;
use signatures::*;
use songs::parse_song_layout;
#[cfg(test)]
mod tests;
mod v1;
mod v2_startup;
pub(crate) mod v3;

const HEADER_BYTES: usize = 32;
pub(super) const MAX_RETAINED_BYTES: usize = 8 * 1024 * 1024;
const MAX_VALIDATION_WORK: u64 = 64_000_000;
const DEFAULT_WORK_RAM: u32 = 0x0300_0000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GaxNativeSong {
    pub header: RomSpan,
    pub index: u16,
    pub title: String,
    pub channels: u16,
    pub native: GaxNativeProfile,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GaxNativeProfile {
    pub version: String,
    pub layout: GaxNativeLayout,
    pub new: Option<GaxNativeEntry>,
    pub init: GaxNativeEntry,
    /// The original frame interrupt routine, which advances audio DMA.
    pub mix: GaxNativeEntry,
    pub play: GaxNativeEntry,
    pub work_ram: u32,
    pub sample_rate: u16,
    pub ram_copies: Vec<GaxRamCopy>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum GaxNativeLayout {
    V2Current,
    V2_01,
    V1_99,
    V3Legacy,
    V3Modern,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GaxRamCopy {
    pub source: RomSpan,
    pub destination: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GaxNativeEntry {
    pub source: RomSpan,
    pub cpu_address: u32,
}

#[derive(Clone, Copy)]
struct Signature {
    bytes: &'static [u8],
}

#[derive(Clone, Copy)]
struct MaskedSignature {
    bytes: &'static [u8],
    mask: &'static [u8],
}

struct Driver {
    version: String,
    layout: GaxNativeLayout,
    new: GaxNativeEntry,
    init: GaxNativeEntry,
    mix: GaxNativeEntry,
    play: GaxNativeEntry,
    work_ram: u32,
    ram_copies: Vec<GaxRamCopy>,
    spans: Vec<RomSpan>,
}

#[derive(Debug)]
pub(super) enum ParseError {
    Invalid,
    Stop(ScanStop),
}

impl From<ScanStop> for ParseError {
    fn from(value: ScanStop) -> Self {
        Self::Stop(value)
    }
}

pub(crate) fn scan(
    bytes: &[u8],
    output: &mut Vec<GaxNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    scan_with_limit(bytes, output, budget, max_candidates, MAX_RETAINED_BYTES)
}

fn scan_with_limit(
    bytes: &[u8],
    output: &mut Vec<GaxNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
    retained_limit: usize,
) -> Result<(), ScanStop> {
    let mut retained = retained_bytes(output);
    if retained > retained_limit {
        return Err(ScanStop::InventoryLimit);
    }
    if let Some(driver) = recognize(bytes, budget)? {
        for offset in (0..=bytes.len().saturating_sub(HEADER_BYTES)).step_by(4) {
            budget.charge()?;
            let (channels, title, mut spans) = match parse_song(bytes, offset, budget) {
                Ok(song) => song,
                Err(ParseError::Invalid) => continue,
                Err(ParseError::Stop(stop)) => return Err(stop),
            };
            if output.len() >= max_candidates || output.len() > u16::MAX as usize {
                return Err(ScanStop::CandidateLimit);
            }
            spans.extend(driver.spans.iter().copied());
            merge_spans(&mut spans);
            let native = GaxNativeProfile {
                version: driver.version.clone(),
                layout: driver.layout,
                new: Some(driver.new),
                init: driver.init,
                mix: driver.mix,
                play: driver.play,
                work_ram: driver.work_ram,
                sample_rate: u16::MAX,
                ram_copies: driver.ram_copies.clone(),
            };
            if driver::workspace(&native).is_none() {
                continue;
            }
            let song = GaxNativeSong {
                header: RomSpan::new(offset, HEADER_BYTES),
                index: output.len() as u16,
                title,
                channels,
                native,
                mapped_spans: spans,
                warnings: Vec::new(),
            };
            push_song(output, song, &mut retained, retained_limit)?;
        }
    }
    v1::scan(bytes, output, budget, max_candidates, retained_limit)?;
    v3::scan(bytes, output, budget, max_candidates, retained_limit)?;
    if retained_bytes(output) > retained_limit {
        return Err(ScanStop::InventoryLimit);
    }
    Ok(())
}

pub fn prepare_rom(
    bytes: &[u8],
    song: &GaxNativeSong,
    cancel: &AtomicBool,
) -> anyhow::Result<Vec<u8>> {
    let current = checked_song(bytes, song, cancel)?;
    match current.native.layout {
        GaxNativeLayout::V3Legacy | GaxNativeLayout::V3Modern => v3::build(bytes, &current),
        GaxNativeLayout::V2Current | GaxNativeLayout::V2_01 | GaxNativeLayout::V1_99 => {
            driver::build(bytes, &current)
        }
    }
}

fn checked_song(
    bytes: &[u8],
    song: &GaxNativeSong,
    cancel: &AtomicBool,
) -> anyhow::Result<GaxNativeSong> {
    ensure!(
        bytes.len() <= MAX_ROM_BYTES,
        "GAX ROM exceeds the size limit"
    );
    let mut budget = Budget {
        cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut songs = Vec::new();
    scan_with_limit(
        bytes,
        &mut songs,
        &mut budget,
        usize::from(u16::MAX) + 1,
        MAX_RETAINED_BYTES,
    )
    .map_err(|stop| anyhow::anyhow!("GAX validation stopped: {stop:?}"))?;
    let current = songs
        .into_iter()
        .find(|candidate| candidate.index == song.index)
        .context("GAX selected song index is no longer available")?;
    ensure!(
        current == *song,
        "GAX selected song or native setup changed"
    );
    Ok(current)
}

fn recognize(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<Driver>, ScanStop> {
    let Some(version) = version(bytes, budget)? else {
        return Ok(None);
    };
    if legacy_v2_layout(&version.0) {
        return recognize_v2_legacy(bytes, version, budget);
    }
    let new = unique_signature(bytes, &NEW_SIGNATURES, budget)?;
    let init = unique_signature(bytes, &INIT_SIGNATURES, budget)?;
    let mix = unique_signature(bytes, &MIX_SIGNATURES, budget)?;
    let play = unique_signature(bytes, &PLAY_SIGNATURES, budget)?;
    let (Some(new), Some(init), Some(mix), Some(play)) = (new, init, mix, play) else {
        return Ok(None);
    };
    if v3::is_init(bytes, init.source.effective_offset as usize) {
        return Ok(None);
    }
    let work_ram =
        work_ram(bytes, play.source.effective_offset as usize).unwrap_or(DEFAULT_WORK_RAM);
    let mut spans = vec![version.1, new.source, init.source, mix.source, play.source];
    if let Some(slot) = work_ram_slot(bytes, play.source.effective_offset as usize) {
        spans.push(RomSpan::new(slot, 4));
    }
    let Some(setup) = v2_startup::inspect(bytes, init, budget)? else {
        return Ok(None);
    };
    spans.extend(setup.spans);
    merge_spans(&mut spans);
    Ok(Some(Driver {
        version: version.0,
        layout: GaxNativeLayout::V2Current,
        new,
        init,
        mix,
        play,
        work_ram,
        ram_copies: setup.copies,
        spans,
    }))
}

fn recognize_v2_legacy(
    bytes: &[u8],
    version: (String, RomSpan),
    budget: &mut Budget<'_>,
) -> Result<Option<Driver>, ScanStop> {
    let new = unique_masked_signature(bytes, &V2_LEGACY_NEW, budget)?;
    let init = unique_masked_signature(bytes, &V2_LEGACY_INIT, budget)?;
    let mix = unique_masked_signature(bytes, &[V2_01_IRQ], budget)?;
    let play = unique_masked_signature(bytes, &V2_01_PLAY, budget)?;
    let (Some(new), Some(init), Some(mix), Some(play)) = (new, init, mix, play) else {
        return Ok(None);
    };
    if !same_legacy_driver([new, init, mix, play]) {
        return Ok(None);
    }
    let (Some((irq_slot, irq_state)), Some((play_slot, play_state))) = (
        state_slot(bytes, mix.source.effective_offset as usize),
        state_slot(bytes, play.source.effective_offset as usize),
    ) else {
        return Ok(None);
    };
    if irq_state != play_state {
        return Ok(None);
    }
    let work_ram = workspace_for_state(play_state).unwrap_or(DEFAULT_WORK_RAM);
    let mut spans = vec![version.1, new.source, init.source, mix.source, play.source];
    spans.extend([RomSpan::new(irq_slot, 4), RomSpan::new(play_slot, 4)]);
    merge_spans(&mut spans);
    Ok(Some(Driver {
        version: version.0,
        layout: GaxNativeLayout::V2_01,
        new,
        init,
        mix,
        play,
        work_ram,
        ram_copies: Vec::new(),
        spans,
    }))
}

fn legacy_v2_layout(version: &str) -> bool {
    ["2.01", "2.02", "2.11"]
        .iter()
        .any(|minor| version.contains(minor))
}

fn same_legacy_driver(entries: [GaxNativeEntry; 4]) -> bool {
    let (min, max) = entries
        .iter()
        .map(|entry| entry.source.effective_offset)
        .fold((u32::MAX, 0), |(min, max), offset| {
            (min.min(offset), max.max(offset))
        });
    max.saturating_sub(min) <= 0x1000
}

pub(super) fn version(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Option<(String, RomSpan)>, ScanStop> {
    const PREFIX: &[u8] = b"GAX Sound Engine ";
    let mut found = None;
    for offset in 0..bytes.len().saturating_sub(PREFIX.len()) {
        if offset % 32 == 0 {
            budget.charge()?;
        }
        if !bytes[offset..].starts_with(PREFIX) {
            continue;
        }
        let end = bytes[offset..bytes.len().min(offset + 128)]
            .iter()
            .position(|byte| *byte == 0)
            .map(|len| offset + len);
        let Some(end) = end else { continue };
        let text = &bytes[offset..end];
        let mut at = PREFIX.len();
        if matches!(text.get(at), Some(b'v' | b'V')) {
            at += 1;
        }
        let Some(&major @ b'1'..=b'2') = text.get(at) else {
            continue;
        };
        if text.get(at + 1) != Some(&b'.') || !text.get(at + 2).is_some_and(u8::is_ascii_digit) {
            continue;
        }
        let candidate = (
            String::from_utf8_lossy(text).into_owned(),
            RomSpan::new(offset, end - offset + 1),
        );
        if found.replace(candidate).is_some() {
            return Ok(None);
        }
        let _ = major;
    }
    Ok(found)
}

fn unique_signature(
    bytes: &[u8],
    signatures: &[Signature],
    budget: &mut Budget<'_>,
) -> Result<Option<GaxNativeEntry>, ScanStop> {
    let mut found = None;
    for signature in signatures {
        for offset in (0..bytes.len()).step_by(2) {
            if offset % 32 == 0 {
                budget.charge()?;
            }
            if bytes.get(offset..offset.saturating_add(signature.bytes.len()))
                != Some(signature.bytes)
            {
                continue;
            }
            let entry = GaxNativeEntry {
                source: RomSpan::new(offset, signature.bytes.len()),
                cpu_address: 0x0800_0001 + offset as u32,
            };
            if found.replace(entry).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(found)
}

fn unique_masked_signature(
    bytes: &[u8],
    signatures: &[MaskedSignature],
    budget: &mut Budget<'_>,
) -> Result<Option<GaxNativeEntry>, ScanStop> {
    let mut found = None;
    for signature in signatures {
        for offset in (0..bytes.len()).step_by(2) {
            if offset % 32 == 0 {
                budget.charge()?;
            }
            let Some(candidate) = bytes.get(offset..offset.saturating_add(signature.bytes.len()))
            else {
                continue;
            };
            if candidate
                .iter()
                .zip(signature.bytes.iter().zip(signature.mask))
                .any(|(actual, (expected, mask))| actual & mask != expected & mask)
            {
                continue;
            }
            let entry = GaxNativeEntry {
                source: RomSpan::new(offset, signature.bytes.len()),
                cpu_address: 0x0800_0001 + offset as u32,
            };
            if found.replace(entry).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(found)
}

fn parse_song(
    bytes: &[u8],
    offset: usize,
    budget: &mut Budget<'_>,
) -> Result<(u16, String, Vec<RomSpan>), ParseError> {
    parse_song_layout(bytes, offset, budget, HEADER_BYTES, 12)
}

fn work_ram(bytes: &[u8], play: usize) -> Option<u32> {
    state_slot(bytes, play).and_then(|(_, state)| workspace_for_state(state))
}

fn work_ram_slot(bytes: &[u8], play: usize) -> Option<usize> {
    state_slot(bytes, play).map(|(slot, _)| slot)
}

fn state_slot(bytes: &[u8], play: usize) -> Option<(usize, u32)> {
    for instruction in (play..play.checked_add(16)?).step_by(2) {
        let opcode = half(bytes, instruction)?;
        if opcode & 0xf800 != 0x4800 {
            continue;
        }
        let base = instruction.checked_add(4)? & !3;
        let slot = base.checked_add(usize::from(opcode & 0xff).checked_mul(4)?)?;
        let value = word(bytes, slot)?;
        if state_pointer(value) {
            return Some((slot, value));
        }
    }
    None
}

fn workspace_for_state(value: u32) -> Option<u32> {
    state_pointer(value).then_some(if (0x0300_0000..0x0300_4000).contains(&value) {
        value + 4
    } else {
        DEFAULT_WORK_RAM
    })
}

pub(super) fn state_pointer(value: u32) -> bool {
    (0x0200_0000..=0x0203_ffff).contains(&value) || (0x0300_0000..=0x0300_7fff).contains(&value)
}

pub(super) fn push_song(
    output: &mut Vec<GaxNativeSong>,
    song: GaxNativeSong,
    retained: &mut usize,
    limit: usize,
) -> Result<(), ScanStop> {
    let owned = song_bytes(&song);
    let available = limit
        .checked_sub(*retained)
        .and_then(|remaining| remaining.checked_sub(owned))
        .ok_or(ScanStop::InventoryLimit)?;
    let mut added_capacity = 0;
    if output.len() == output.capacity() {
        if available < size_of::<GaxNativeSong>() {
            return Err(ScanStop::InventoryLimit);
        }
        let previous = output.capacity();
        output
            .try_reserve_exact(1)
            .map_err(|_| ScanStop::InventoryLimit)?;
        added_capacity = (output.capacity() - previous) * size_of::<GaxNativeSong>();
        if added_capacity > available {
            return Err(ScanStop::InventoryLimit);
        }
    }
    *retained += owned + added_capacity;
    output.push(song);
    Ok(())
}

pub(super) fn retained_bytes(songs: &Vec<GaxNativeSong>) -> usize {
    songs.iter().map(song_bytes).fold(
        songs.capacity().saturating_mul(size_of::<GaxNativeSong>()),
        usize::saturating_add,
    )
}

fn song_bytes(song: &GaxNativeSong) -> usize {
    song.title
        .capacity()
        .saturating_add(song.mapped_spans.capacity() * size_of::<RomSpan>())
        .saturating_add(song.warnings.iter().map(String::capacity).sum::<usize>())
        .saturating_add(song.warnings.capacity() * size_of::<String>())
        .saturating_add(song.native.version.capacity())
        .saturating_add(
            song.native
                .ram_copies
                .capacity()
                .saturating_mul(size_of::<GaxRamCopy>()),
        )
}

fn half(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

pub(super) fn pointer(bytes: &[u8], slot: usize, len: usize, align: usize) -> Option<usize> {
    rom_pointer(bytes, word(bytes, slot)?, len, align)
}

fn merge_spans(spans: &mut Vec<RomSpan>) {
    spans.sort_unstable_by_key(|span| (span.effective_offset, span.byte_len));
    spans.dedup();
}

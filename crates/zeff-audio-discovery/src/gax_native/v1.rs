use std::ops::Range;

use anyhow::ensure;

use super::{
    Budget, GaxNativeEntry, GaxNativeLayout, GaxNativeProfile, GaxNativeSong, ParseError, RomSpan,
    ScanStop, half, merge_spans, parse_song_layout, pointer, push_song, retained_bytes,
    state_pointer, version, word,
};

mod signatures;
#[cfg(test)]
mod tests;

const MIXER_REQUEST_HZ: u16 = 16_000;

struct Profile {
    native: GaxNativeProfile,
    header_bytes: usize,
    spans: Vec<RomSpan>,
}

pub(super) fn scan(
    bytes: &[u8],
    output: &mut Vec<GaxNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
    retained_limit: usize,
) -> Result<(), ScanStop> {
    let Some(profile) = recognize(bytes, budget)? else {
        return Ok(());
    };
    let mut retained = retained_bytes(output);
    if retained > retained_limit {
        return Err(ScanStop::InventoryLimit);
    }
    for offset in (0..bytes.len().saturating_sub(12)).step_by(4) {
        budget.charge()?;
        let (channels, title, mut spans) = match parse_song_layout(
            bytes,
            offset,
            budget,
            profile.header_bytes,
            profile.header_bytes - 12,
        ) {
            Ok(song) => song,
            Err(ParseError::Invalid) => continue,
            Err(ParseError::Stop(stop)) => return Err(stop),
        };
        if !channel_links(bytes, offset, channels, profile.header_bytes, budget)? {
            continue;
        }
        if output.len() >= max_candidates || output.len() > u16::MAX as usize {
            return Err(ScanStop::CandidateLimit);
        }
        spans.extend(profile.spans.iter().copied());
        merge_spans(&mut spans);
        let song = GaxNativeSong {
            header: RomSpan::new(offset, (usize::from(channels) + profile.header_bytes / 4 - 2) * 4),
            index: output.len() as u16,
            title,
            channels,
            native: profile.native.clone(),
            mapped_spans: spans,
            warnings: vec!["GAX 1 headers contain no mixer rate; playback requests a 16 kHz native mixer rate before output resampling.".to_owned()],
        };
        push_song(output, song, &mut retained, retained_limit)?;
    }
    Ok(())
}

fn recognize(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<Profile>, ScanStop> {
    let mut found = None;
    for at in (0..bytes.len()).step_by(2) {
        if at.is_multiple_of(32) {
            budget.charge()?;
        }
        for (pattern, header_bytes, state_at) in [
            (&signatures::INIT_OLD, 20, 30),
            (&signatures::INIT_NEW, 24, 34),
        ] {
            if !pattern.matches(bytes, at) {
                continue;
            }
            let Some((state, slot)) = literal(bytes, at + state_at, 6) else {
                continue;
            };
            if !state_pointer(state) || !state.is_multiple_of(4) {
                continue;
            }
            if found
                .replace((at, header_bytes, pattern.code.len(), state, slot))
                .is_some()
            {
                return Ok(None);
            }
        }
    }
    let Some((init_at, header_bytes, init_length, state, state_slot)) = found else {
        return Ok(None);
    };
    let Some((version, version_span)) = version(bytes, budget)? else {
        return Ok(None);
    };
    if !version
        .split_ascii_whitespace()
        .nth(3)
        .is_some_and(|number| number.trim_start_matches(['v', 'V']).starts_with("1."))
    {
        return Ok(None);
    }
    let region = init_at + init_length..bytes.len().min(init_at + 0x1200);
    let Some((irq, irq_slots)) = entry(bytes, region.clone(), &signatures::IRQ, state, budget)?
    else {
        return Ok(None);
    };
    let play_pattern = if header_bytes == 20 {
        &signatures::PLAY_OLD
    } else {
        &signatures::PLAY_NEW
    };
    let Some((play, play_slots)) = entry(bytes, region, play_pattern, state, budget)? else {
        return Ok(None);
    };
    let init = GaxNativeEntry {
        source: RomSpan::new(init_at, init_length),
        cpu_address: 0x0800_0001 + init_at as u32,
    };
    let mut spans = vec![
        init.source,
        irq.source,
        play.source,
        state_slot,
        version_span,
    ];
    spans.extend(irq_slots);
    spans.extend(play_slots);
    let native = GaxNativeProfile {
        version,
        layout: GaxNativeLayout::V1_99,
        new: None,
        init,
        mix: irq,
        play,
        work_ram: state,
        sample_rate: MIXER_REQUEST_HZ,
        ram_copies: Vec::new(),
    };
    if workspace(&native).is_none() {
        return Ok(None);
    }
    Ok(Some(Profile {
        native,
        header_bytes,
        spans,
    }))
}

fn entry(
    bytes: &[u8],
    region: Range<usize>,
    pattern: &signatures::Pattern,
    state: u32,
    budget: &mut Budget<'_>,
) -> Result<Option<(GaxNativeEntry, Vec<RomSpan>)>, ScanStop> {
    let mut found = None;
    for at in region.step_by(2) {
        budget.charge()?;
        if !pattern.matches(bytes, at) {
            continue;
        }
        let register = if pattern.irq { 4 } else { 5 };
        let Some((pointer, state_slot)) = literal(bytes, at + 2, register) else {
            continue;
        };
        if pointer != state {
            continue;
        }
        let mut spans = vec![state_slot];
        if pattern.irq {
            for (offset, register, expected) in [(30, 1, 0x0400_0084), (46, 2, 0x0400_0100)] {
                let Some((value, slot)) = literal(bytes, at + offset, register) else {
                    return Ok(None);
                };
                if value != expected {
                    return Ok(None);
                }
                spans.push(slot);
            }
        } else {
            let call = at + pattern.call_offset;
            if half(bytes, call).is_none_or(|opcode| opcode & 0xf800 != 0xf000)
                || half(bytes, call + 2).is_none_or(|opcode| opcode & 0xf800 != 0xf800)
            {
                continue;
            }
            if pattern.call_offset == 42 {
                let Some((value, slot)) = literal(bytes, at + 16, 1) else {
                    continue;
                };
                if value != 0x202 {
                    continue;
                }
                spans.push(slot);
            }
        }
        let candidate = GaxNativeEntry {
            source: RomSpan::new(at, pattern.code.len()),
            cpu_address: 0x0800_0001 + at as u32,
        };
        if found.replace((candidate, spans)).is_some() {
            return Ok(None);
        }
    }
    Ok(found)
}

fn literal(bytes: &[u8], at: usize, register: u16) -> Option<(u32, RomSpan)> {
    let instruction = half(bytes, at)?;
    if instruction & 0xff00 != 0x4800 | register << 8 {
        return None;
    }
    let slot = ((at + 4) & !3).checked_add(usize::from(instruction & 255) * 4)?;
    Some((word(bytes, slot)?, RomSpan::new(slot, 4)))
}

fn channel_links(
    bytes: &[u8],
    offset: usize,
    channels: u16,
    header_bytes: usize,
    budget: &mut Budget<'_>,
) -> Result<bool, ScanStop> {
    let extra = header_bytes / 4 - 3;
    if word(bytes, offset) != Some(u32::from(channels) + extra as u32) {
        return Ok(false);
    }
    let Some(patterns) = pointer(bytes, offset + 4, 28, 4) else {
        return Ok(false);
    };
    if word(bytes, patterns + 12) != Some(u32::from(channels)) {
        return Ok(false);
    }
    let Some(linked) = pointer(bytes, patterns + 16, usize::from(channels) * 4, 4) else {
        return Ok(false);
    };
    for index in 0..usize::from(channels) {
        budget.charge()?;
        if word(bytes, linked + index * 4) != word(bytes, offset + 4 + (index + extra) * 4) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn workspace(profile: &GaxNativeProfile) -> Option<(u32, u32)> {
    let mut reserved = vec![(0x0300_0000, 0x0300_00a0), (0x0300_7d00, 0x0300_8000)];
    if (0x0300_0000..0x0300_8000).contains(&profile.work_ram) {
        reserved.push((profile.work_ram, profile.work_ram.checked_add(4)?));
    }
    for copy in &profile.ram_copies {
        if (0x0300_0000..0x0300_8000).contains(&copy.destination) {
            reserved.push((
                copy.destination,
                copy.destination.checked_add(copy.source.byte_len)?,
            ));
        }
    }
    reserved.sort_unstable();
    let mut end = 0x0300_0000;
    let mut best = (0, 0);
    for (low, high) in reserved {
        let start = (end + 31) & !31;
        let length = low.saturating_sub(start).min(0x6000) & !3;
        if length > best.1 {
            best = (start, length);
        }
        end = end.max(high);
    }
    (best.1 >= 0x2000).then_some(best)
}

pub(super) fn initialize(
    code: &mut super::driver::Program,
    song: &GaxNativeSong,
) -> anyhow::Result<()> {
    let (work, length) =
        workspace(&song.native).ok_or_else(|| anyhow::anyhow!("GAX 1 has no bounded work area"))?;
    ensure!(
        song.native.new.is_none() && song.native.sample_rate == MIXER_REQUEST_HZ,
        "GAX 1 bootstrap parameters changed"
    );
    code.emit(0xe3a0_0000);
    code.emit(0xe58d_0000);
    code.emit(0xe58d_0004);
    code.literal(0, work);
    code.literal(1, length);
    code.literal(2, song.header.canonical_cpu_address);
    code.literal(3, u32::from(song.native.sample_rate));
    code.call(song.native.init.cpu_address);
    Ok(())
}

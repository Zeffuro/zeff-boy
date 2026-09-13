use std::collections::BTreeSet;

use super::{
    Budget, DescriptorMidiNativeProfile, DescriptorMidiSong, MAX_EVENTS, ReadError, ReadResult,
    ScanStop, assets::Assets, half, pointer, span, word,
};

pub(super) fn parse_song(
    bytes: &[u8],
    native: &DescriptorMidiNativeProfile,
    index: u16,
    budget: &mut Budget<'_>,
) -> ReadResult<DescriptorMidiSong> {
    budget.charge()?;
    if usize::from(index) >= native.song_table.byte_len as usize / 8 {
        return Err(ReadError::Invalid);
    }
    let slot = native.song_table.effective_offset as usize + usize::from(index) * 8;
    let descriptor = pointer(bytes, slot, 20, 4)?;
    let player = half(bytes, slot + 4).ok_or(ReadError::Invalid)? as usize;
    let state = native.players.get(player).ok_or(ReadError::Invalid)?;
    let flags = word(bytes, descriptor + 4).ok_or(ReadError::Invalid)?;
    if word(bytes, descriptor + 16) != Some(u32::from(index)) || flags & 31 != player as u32 {
        return Err(ReadError::Invalid);
    }
    let midi = pointer(bytes, descriptor, 14, 4)?;
    let name = pointer(bytes, descriptor + 12, 1, 1)?;
    let name_length = bytes[name..]
        .iter()
        .take(128)
        .position(|&c| c == 0)
        .ok_or(ReadError::Invalid)?;
    let title_bytes = &bytes[name..name + name_length];
    if title_bytes.is_empty()
        || !title_bytes
            .iter()
            .all(|c| c.is_ascii_graphic() || *c == b' ')
    {
        return Err(ReadError::Invalid);
    }
    let bank_slot =
        native.bank_table.effective_offset as usize + ((flags as usize & 0x7fff) >> 5) * 4;
    let bank = pointer(bytes, bank_slot, 4, 4)?;
    let mut assets = Assets::new();
    for (at, length) in [
        (slot, 8),
        (descriptor, 20),
        (name, name_length + 1),
        (bank_slot, 4),
    ] {
        assets.add(bytes, at, length)?;
    }
    let parsed = midi_events(bytes, midi, state.channels, budget)?;
    for &(program, key) in &parsed.voices {
        assets.voice(bytes, bank, program, key, budget)?;
    }
    if assets.instruments.is_empty() || parsed.channels == 0 {
        return Err(ReadError::Invalid);
    }
    let midi_span = span(bytes, midi, parsed.length)?;
    assets.spans.insert(midi_span);
    assets.spans.extend(native.setup_spans.iter().copied());
    let mut warnings = vec![
        "Runs the original descriptor MIDI driver with the source's initialization settings in an isolated GBA emulator.".to_owned(),
        "Native playback assigns channels by track. MIDI export preserves source bytes; other MIDI players need the matching bank and can interpret channels differently.".to_owned(),
    ];
    if parsed.max_velocity <= 1 {
        warnings
            .push("Sequence notes use minimum velocity; native playback can be silent.".to_owned());
    }
    if parsed.tracks > u16::from(state.channels) {
        warnings.push(format!(
            "The source's native player uses the first {} of {} stored MIDI tracks.",
            state.channels, parsed.tracks
        ));
    }
    Ok(DescriptorMidiSong {
        root: native.song_table,
        header: span(bytes, descriptor, 20)?,
        midi: midi_span,
        index,
        title: String::from_utf8(title_bytes.to_vec()).map_err(|_| ReadError::Invalid)?,
        channels: parsed.channels,
        tracks: parsed.tracks,
        notes: parsed.notes,
        instruments: assets.instruments.len() as u16,
        samples: assets.samples.len() as u16,
        native: native.clone(),
        mapped_spans: assets.spans.into_iter().collect(),
        warnings,
    })
}

struct ParsedMidi {
    length: usize,
    tracks: u16,
    channels: u8,
    notes: u32,
    max_velocity: u8,
    voices: BTreeSet<(u8, u8)>,
}

fn midi_events(
    bytes: &[u8],
    at: usize,
    max_tracks: u8,
    budget: &mut Budget<'_>,
) -> ReadResult<ParsedMidi> {
    if bytes.get(at..at + 4) != Some(b"MThd") || be32(bytes, at + 4)? != 6 {
        return Err(ReadError::Invalid);
    }
    let format = be16(bytes, at + 8)?;
    let tracks = be16(bytes, at + 10)?;
    let division = be16(bytes, at + 12)?;
    if format > 1
        || (format == 0 && tracks != 1)
        || tracks == 0
        || tracks > 31
        || division == 0
        || division >= 0x8000
    {
        return Err(ReadError::Invalid);
    }
    let mut parsed = ParsedMidi {
        length: 0,
        tracks,
        channels: 0,
        notes: 0,
        max_velocity: 0,
        voices: BTreeSet::new(),
    };
    let mut pos = at + 14;
    let mut events = 0;
    let mut loop_start = None;
    let mut loop_end = None;
    let mut max_step = 1;
    let mut max_delta = 0;
    let mut track_voices = Vec::new();
    for track_index in 0..tracks {
        if bytes.get(pos..pos + 4) != Some(b"MTrk") {
            return Err(ReadError::Invalid);
        }
        let length = be32(bytes, pos + 4)? as usize;
        pos += 8;
        let end = pos
            .checked_add(length)
            .filter(|&v| v <= bytes.len())
            .ok_or(ReadError::Invalid)?;
        let mut track = Track::default();
        let mut ended = false;
        while pos < end {
            budget.charge()?;
            events += 1;
            if events > MAX_EVENTS {
                return Err(ReadError::Stop(ScanStop::ValidationLimit));
            }
            let delta = vlq(bytes, &mut pos, end)?;
            if delta > u32::MAX >> 8 {
                return Err(ReadError::Invalid);
            }
            max_delta = max_delta.max(delta);
            track.ticks = track.ticks.checked_add(delta).ok_or(ReadError::Invalid)?;
            let first = read(bytes, &mut pos, end)?;
            let status = if first >= 0x80 {
                track.status = first;
                first
            } else {
                pos -= 1;
                track.status
            };
            match status {
                0x80..=0xef => {
                    let first = read(bytes, &mut pos, end)?;
                    let kind = status & 0xf0;
                    let second = if kind == 0xc0 || kind == 0xd0 {
                        0
                    } else {
                        read(bytes, &mut pos, end)?
                    };
                    if first >= 128 || second >= 128 {
                        return Err(ReadError::Invalid);
                    }
                    if kind == 0xc0 {
                        track.program = first;
                        track.events.push(VoiceEvent {
                            tick: track.ticks,
                            value: first,
                            program: true,
                        });
                    } else if kind == 0x90 && second != 0 {
                        track.sounding = true;
                        parsed.notes += 1;
                        parsed.max_velocity = parsed.max_velocity.max(second);
                        parsed.voices.insert((track.program, first));
                        track.events.push(VoiceEvent {
                            tick: track.ticks,
                            value: first,
                            program: false,
                        });
                    }
                }
                0xff => {
                    let kind = read(bytes, &mut pos, end)?;
                    let length = vlq(bytes, &mut pos, end)? as usize;
                    let finish = pos
                        .checked_add(length)
                        .filter(|&v| v <= end)
                        .ok_or(ReadError::Invalid)?;
                    match kind {
                        0x2f => {
                            if length != 0 || finish != end {
                                return Err(ReadError::Invalid);
                            }
                            ended = true;
                        }
                        0x51 => {
                            if length != 3 || bytes[pos..finish] == [0, 0, 0] {
                                return Err(ReadError::Invalid);
                            }
                            let tempo =
                                u32::from_be_bytes([0, bytes[pos], bytes[pos + 1], bytes[pos + 2]]);
                            let bpm = u32::from((60_000_000 / tempo) as u16);
                            let step =
                                bpm.wrapping_mul(256).wrapping_mul(u32::from(division)) / 3600;
                            max_step = max_step.max(step);
                        }
                        6 if track_index < u16::from(max_tracks) && bytes[pos..finish] == *b"[" => {
                            loop_start =
                                Some(loop_start.map_or(track.ticks, |v: u32| v.min(track.ticks)));
                        }
                        6 if track_index < u16::from(max_tracks) && bytes[pos..finish] == *b"]" => {
                            loop_end =
                                Some(loop_end.map_or(track.ticks, |v: u32| v.max(track.ticks)));
                        }
                        _ => {}
                    }
                    pos = finish;
                }
                0xf0 | 0xf7 => {
                    let length = vlq(bytes, &mut pos, end)? as usize;
                    let finish = pos
                        .checked_add(length)
                        .filter(|&v| v <= end)
                        .ok_or(ReadError::Invalid)?;
                    // The native F0 handler reads an engine command and its fixed payload.
                    if status == 0xf0 {
                        let command = *bytes
                            .get(pos)
                            .filter(|_| length != 0)
                            .ok_or(ReadError::Invalid)?;
                        if (command == 0 && length < 8) || (command == 1 && length < 13) {
                            return Err(ReadError::Invalid);
                        }
                    }
                    pos = finish;
                }
                _ => return Err(ReadError::Invalid),
            }
        }
        if !ended {
            return Err(ReadError::Invalid);
        }
        parsed.channels += u8::from(track.sounding && track_index < u16::from(max_tracks));
        track_voices.push(track.events);
    }
    // Native track delays add eight fractional bits and a residual frame step.
    max_delta
        .checked_mul(256)
        .and_then(|delay| delay.checked_add(max_step))
        .ok_or(ReadError::Invalid)?;
    if loop_start.is_some() || loop_end.is_some() {
        if loop_end.is_some_and(|end| loop_start.is_none_or(|start| end <= start)) {
            return Err(ReadError::Invalid);
        }
        let margin = max_step.div_ceil(256).saturating_add(1).saturating_mul(2);
        let from = loop_start.unwrap_or(0).saturating_sub(margin);
        let through = loop_end.unwrap_or(u32::MAX).saturating_add(margin);
        close_loop_voices(&mut parsed.voices, track_voices, from, through, budget)?;
    }
    parsed.length = pos - at;
    Ok(parsed)
}

#[derive(Default)]
struct Track {
    status: u8,
    program: u8,
    ticks: u32,
    sounding: bool,
    events: Vec<VoiceEvent>,
}

struct VoiceEvent {
    tick: u32,
    value: u8,
    program: bool,
}

fn close_loop_voices(
    voices: &mut BTreeSet<(u8, u8)>,
    tracks: Vec<Vec<VoiceEvent>>,
    from: u32,
    through: u32,
    budget: &mut Budget<'_>,
) -> ReadResult<()> {
    // Native loop snapshots use frame boundaries and retain channel programs.
    for events in tracks {
        let mut current = 0;
        let mut programs = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for event in events {
            budget.charge()?;
            if event.tick > through {
                break;
            }
            if event.tick < from {
                if event.program {
                    current = event.value;
                }
            } else if event.program {
                programs.insert(event.value);
            } else {
                keys.insert(event.value);
            }
        }
        programs.insert(current);
        for program in programs {
            for &key in &keys {
                budget.charge()?;
                voices.insert((program, key));
            }
        }
    }
    Ok(())
}

fn read(bytes: &[u8], pos: &mut usize, end: usize) -> ReadResult<u8> {
    if *pos >= end {
        return Err(ReadError::Invalid);
    }
    let value = *bytes.get(*pos).ok_or(ReadError::Invalid)?;
    *pos += 1;
    Ok(value)
}

fn vlq(bytes: &[u8], pos: &mut usize, end: usize) -> ReadResult<u32> {
    let mut value = 0;
    for _ in 0..4 {
        let byte = read(bytes, pos, end)?;
        value = (value << 7) | u32::from(byte & 127);
        if byte < 128 {
            return Ok(value);
        }
    }
    Err(ReadError::Invalid)
}

fn be16(bytes: &[u8], at: usize) -> ReadResult<u16> {
    let value = bytes
        .get(at..at.checked_add(2).ok_or(ReadError::Invalid)?)
        .ok_or(ReadError::Invalid)?;
    Ok(u16::from_be_bytes([value[0], value[1]]))
}

fn be32(bytes: &[u8], at: usize) -> ReadResult<u32> {
    let value = bytes
        .get(at..at.checked_add(4).ok_or(ReadError::Invalid)?)
        .ok_or(ReadError::Invalid)?;
    Ok(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
}

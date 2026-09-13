use std::collections::{BTreeMap, BTreeSet};

use super::{
    Budget, FileSpan, GbSong, GbTermination, GbWarning, MAX_EVENTS, ScanStop, pointer, span,
};

#[derive(Debug)]
pub enum ReadError {
    Invalid(String),
    Stop(ScanStop),
}

impl From<ScanStop> for ReadError {
    fn from(value: ScanStop) -> Self {
        Self::Stop(value)
    }
}

pub(crate) fn error(error: impl Into<ReadError>) -> anyhow::Error {
    match error.into() {
        ReadError::Invalid(reason) => anyhow::anyhow!("Game Boy music validation failed: {reason}"),
        ReadError::Stop(stop) => anyhow::anyhow!("Game Boy music validation stopped: {stop:?}"),
    }
}

pub struct Program {
    pub song: GbSong,
    pub notes: Vec<Note>,
    pub truncated: bool,
}

pub struct Note {
    pub channel: u8,
    pub frame: u32,
    pub duration: u32,
    pub frequency: u16,
    pub drum: Option<(u8, u8)>,
    pub volume: u8,
    pub pan: u8,
}

struct Channel {
    pc: u16,
    next_frame: u32,
    done: bool,
    octave: u8,
    transpose: u8,
    speed: u8,
    envelope: u8,
    tempo: u16,
    remainder: u8,
    pitch_offset: u16,
    noise: Option<u8>,
    pan: u8,
    enabled: bool,
    return_address: Option<u16>,
    loop_count: Option<u8>,
    passes: u8,
    visits: BTreeMap<u16, u32>,
    cycles: BTreeMap<ControlState, u32>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct ControlState {
    pc: u16,
    return_address: Option<u16>,
    loop_count: Option<u8>,
    octave: u8,
    transpose: u8,
    speed: u8,
    envelope: u8,
    tempo: u16,
    pitch_offset: u16,
    noise: Option<u8>,
    pan: u8,
    enabled: bool,
    volume: u8,
}

impl Channel {
    fn new(pc: u16, passes: u8) -> Self {
        Self {
            pc,
            passes,
            next_frame: 0,
            done: false,
            octave: 0,
            transpose: 0,
            speed: 1,
            envelope: 0,
            tempo: 256,
            remainder: 0,
            pitch_offset: 0,
            noise: None,
            pan: 64,
            enabled: true,
            return_address: None,
            loop_count: None,
            visits: BTreeMap::new(),
            cycles: BTreeMap::new(),
        }
    }

    fn control_state(&self, volume: u8) -> ControlState {
        ControlState {
            pc: self.pc,
            return_address: self.return_address,
            loop_count: self.loop_count,
            octave: self.octave,
            transpose: self.transpose,
            speed: self.speed,
            envelope: self.envelope,
            tempo: self.tempo,
            pitch_offset: self.pitch_offset,
            noise: self.noise,
            pan: self.pan,
            enabled: self.enabled,
            volume,
        }
    }
}

pub(crate) fn interpret(
    bytes: &[u8],
    mut song: GbSong,
    loops: u8,
    max_frames: u32,
    budget: &mut Budget<'_>,
) -> Result<Program, ReadError> {
    let mut channels = song
        .channels
        .iter()
        .map(|channel| Channel::new(channel.cpu_address, loops))
        .collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut notes = Vec::new();
    let mut truncated = false;
    let mut global_volume: u8 = 0x77;
    let mut events = 0;
    for channel in &mut song.channels {
        channel.event_count = 0;
        channel.note_count = 0;
        channel.end_frame = 0;
        channel.loop_start_frame = None;
        channel.termination = GbTermination::Unresolved;
    }
    song.warnings.clear();
    'playback: while let Some(index) = channels
        .iter()
        .enumerate()
        .filter(|(_, channel)| !channel.done)
        .min_by_key(|(index, channel)| (channel.next_frame, *index))
        .map(|(index, _)| index)
    {
        budget.charge()?;
        let frame = channels[index].next_frame;
        if frame >= max_frames {
            truncated = true;
            finish(&mut song, &mut notes, max_frames);
            break;
        }
        let mut zero_time = BTreeSet::new();
        loop {
            budget.charge()?;
            events += 1;
            let pc = channels[index].pc;
            let offset = pointer(bytes, song.bank, pc, 1)?;
            if events > MAX_EVENTS {
                return Err(ReadError::Stop(ScanStop::ValidationLimit));
            }
            let key = channels[index].control_state(global_volume);
            if let Some(&start) = channels[index].cycles.get(&key)
                && start < frame
            {
                song.channels[index].loop_start_frame.get_or_insert(start);
                song.channels[index].termination = GbTermination::Loop;
                channels[index].passes = channels[index].passes.saturating_sub(1);
                if complete(&channels) {
                    finish(&mut song, &mut notes, frame);
                    break 'playback;
                }
                channels[index].cycles.clear();
            }
            // A repeating musical section retains its fractional frame carry across passes.
            channels[index].cycles.insert(key, frame);
            let state = &channels[index];
            if !zero_time.insert((pc, state.return_address, state.loop_count)) {
                unresolved(
                    &mut song,
                    &mut channels,
                    index,
                    offset,
                    "zero-duration control-flow cycle",
                );
                break;
            }
            channels[index].visits.entry(pc).or_insert(frame);
            song.channels[index].event_count += 1;
            let opcode = take(bytes, song.bank, &mut channels[index].pc, 1, &mut visited)?[0];
            if opcode < 0xd0 {
                let state = &mut channels[index];
                let product =
                    u16::from(state.speed).wrapping_mul(u16::from((opcode & 15) + 1)) as u8;
                let duration = u16::from(product)
                    .wrapping_mul(state.tempo)
                    .wrapping_add(u16::from(state.remainder));
                state.remainder = duration as u8;
                let duration = u32::from((duration >> 8).max(1));
                let pitch = opcode >> 4;
                if pitch != 0 {
                    let number = song.channels[index].number;
                    let frequency = if number == 4 {
                        if state.noise.is_none() {
                            unresolved(
                                &mut song,
                                &mut channels,
                                index,
                                offset,
                                "noise note without a selected drum kit",
                            );
                            break;
                        }
                        0
                    } else {
                        match frequency(state, pitch) {
                            Some(value) => value,
                            None => {
                                unresolved(
                                    &mut song,
                                    &mut channels,
                                    index,
                                    offset,
                                    "pitch transposition leaves the verified note table",
                                );
                                break;
                            }
                        }
                    };
                    let volume = if number == 4 {
                        100
                    } else if number == 3 {
                        // The driver shifts the envelope into NR32's two volume bits.
                        [0, 127, 64, 32][usize::from((state.envelope >> 4) & 3)]
                    } else {
                        (u16::from(state.envelope >> 4) * 127 / 15) as u8
                    };
                    let master = ((global_volume >> 4) & 7).max(global_volume & 7);
                    notes.push(Note {
                        channel: number,
                        frame,
                        duration: duration.min(max_frames - frame),
                        frequency,
                        drum: if number == 4 {
                            state.noise.map(|kit| (kit, pitch))
                        } else {
                            None
                        },
                        volume: if state.enabled {
                            (u16::from(volume) * u16::from(master) / 7) as u8
                        } else {
                            0
                        },
                        pan: state.pan,
                    });
                    song.channels[index].note_count += 1;
                }
                channels[index].next_frame = frame + duration;
                song.channels[index].end_frame = (frame + duration).min(max_frames);
                break;
            }
            let count = match opcode {
                0xd0..=0xd7 | 0xec..=0xed | 0xf1..=0xf8 | 0xff => 0,
                0xd8 => {
                    if song.channels[index].number == 4 {
                        1
                    } else {
                        2
                    }
                }
                0xd9 | 0xdb..=0xde | 0xe4..=0xe5 | 0xe9 | 0xef => 1,
                0xda | 0xe1 | 0xe6 | 0xfc | 0xfe => 2,
                0xe3 => usize::from(channels[index].noise.is_none()),
                0xfd => 3,
                _ => {
                    unresolved(
                        &mut song,
                        &mut channels,
                        index,
                        offset,
                        &format!("unsupported command 0x{opcode:02X}"),
                    );
                    break;
                }
            };
            let args = take(
                bytes,
                song.bank,
                &mut channels[index].pc,
                count,
                &mut visited,
            )?;
            match opcode {
                0xd0..=0xd7 => channels[index].octave = opcode & 7,
                0xd8 => {
                    if !(1..=15).contains(&args[0]) {
                        unresolved(
                            &mut song,
                            &mut channels,
                            index,
                            offset,
                            "invalid note length",
                        );
                        break;
                    }
                    channels[index].speed = args[0];
                    if args.len() > 1 {
                        channels[index].envelope = args[1];
                    }
                }
                0xd9 => channels[index].transpose = args[0],
                0xda | 0xe9 => {
                    let tempo = if opcode == 0xda {
                        u16::from_be_bytes([args[0], args[1]])
                    } else {
                        channels[index]
                            .tempo
                            .wrapping_add_signed(i16::from(args[0] as i8))
                    };
                    for state in &mut channels {
                        state.tempo = tempo;
                        state.remainder = 0;
                    }
                }
                0xdc => channels[index].envelope = args[0],
                0xdd if args[0] != 0 => {
                    unresolved(
                        &mut song,
                        &mut channels,
                        index,
                        offset,
                        "hardware pitch sweep is not projected to MIDI",
                    );
                    break;
                }
                0xe3 => {
                    if song.channels[index].number != 4 || (!args.is_empty() && args[0] >= 6) {
                        unresolved(
                            &mut song,
                            &mut channels,
                            index,
                            offset,
                            "unverified noise mode or drum kit",
                        );
                        break;
                    }
                    channels[index].noise = args.first().copied();
                }
                0xe4 | 0xef => {
                    channels[index].pan = match (args[0] & 0xf0 != 0, args[0] & 0x0f != 0) {
                        (true, false) => 0,
                        (false, true) => 127,
                        _ => 64,
                    };
                    channels[index].enabled = args[0] != 0;
                }
                0xe5 => global_volume = args[0],
                0xe6 => channels[index].pitch_offset = u16::from_be_bytes([args[0], args[1]]),
                0xfc..=0xfe => {
                    let target = u16::from_le_bytes([args[count - 2], args[count - 1]]);
                    pointer(bytes, song.bank, target, 1)?;
                    if opcode == 0xfe {
                        if channels[index].return_address.is_some() {
                            unresolved(
                                &mut song,
                                &mut channels,
                                index,
                                offset,
                                "nested calls overwrite the driver's single return address",
                            );
                            break;
                        }
                        channels[index].return_address = Some(channels[index].pc);
                        channels[index].pc = target;
                    } else if opcode == 0xfd && args[0] != 0 {
                        let left = channels[index].loop_count.unwrap_or(args[0] - 1);
                        if left == 0 {
                            channels[index].loop_count = None;
                        } else {
                            channels[index].loop_count = Some(left - 1);
                            channels[index].pc = target;
                        }
                    } else if let Some(&start) = channels[index].visits.get(&target) {
                        if start == frame {
                            unresolved(
                                &mut song,
                                &mut channels,
                                index,
                                offset,
                                "infinite loop has no duration",
                            );
                            break;
                        }
                        song.channels[index].loop_start_frame.get_or_insert(start);
                        song.channels[index].termination = GbTermination::Loop;
                        channels[index].passes = channels[index].passes.saturating_sub(1);
                        if complete(&channels) {
                            finish(&mut song, &mut notes, frame);
                            break 'playback;
                        }
                        channels[index].cycles.clear();
                        channels[index].pc = target;
                    } else {
                        channels[index].pc = target;
                    }
                }
                0xff => {
                    if let Some(target) = channels[index].return_address.take() {
                        channels[index].pc = target;
                    } else {
                        channels[index].done = true;
                        song.channels[index].termination = GbTermination::Fine;
                        song.channels[index].end_frame = frame;
                        if complete(&channels) {
                            finish(&mut song, &mut notes, frame);
                            break 'playback;
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    song.mapped_spans = mapped(&visited, &[song.table_entry, song.header]);
    song.midi_exportable = !truncated
        && song
            .channels
            .iter()
            .all(|channel| channel.termination != GbTermination::Unresolved);
    Ok(Program {
        song,
        notes,
        truncated,
    })
}

fn complete(channels: &[Channel]) -> bool {
    channels.iter().all(|state| state.done || state.passes == 0)
}

fn finish(song: &mut GbSong, notes: &mut Vec<Note>, frame: u32) {
    for channel in &mut song.channels {
        if channel.termination == GbTermination::Loop || channel.end_frame > frame {
            channel.end_frame = frame;
        }
    }
    notes.retain_mut(|note| {
        note.duration = note.duration.min(frame.saturating_sub(note.frame));
        note.duration != 0
    });
    for channel in &mut song.channels {
        channel.note_count = notes
            .iter()
            .filter(|note| note.channel == channel.number)
            .count() as u32;
    }
}

fn take<'a>(
    bytes: &'a [u8],
    bank: u8,
    pc: &mut u16,
    count: usize,
    visited: &mut BTreeSet<usize>,
) -> Result<&'a [u8], ReadError> {
    if count == 0 {
        return Ok(&[]);
    }
    let offset = pointer(bytes, bank, *pc, count)?;
    visited.extend(offset..offset + count);
    *pc += count as u16;
    Ok(&bytes[offset..offset + count])
}

fn unresolved(
    song: &mut GbSong,
    channels: &mut [Channel],
    index: usize,
    offset: usize,
    reason: &str,
) {
    song.warnings.push(GbWarning {
        offset: offset as u32,
        reason: reason.into(),
    });
    channels[index].done = true;
    song.channels[index].end_frame = channels[index].next_frame;
}

fn mapped(visited: &BTreeSet<usize>, extra: &[FileSpan]) -> Vec<FileSpan> {
    let mut bytes = visited.clone();
    for range in extra {
        bytes.extend(range.offset as usize..(range.offset + range.byte_len) as usize);
    }
    let mut spans: Vec<FileSpan> = Vec::new();
    for offset in bytes {
        if let Some(last) = spans.last_mut()
            && last.offset + last.byte_len == offset as u32
        {
            last.byte_len += 1;
        } else {
            spans.push(span(offset, 1));
        }
    }
    spans
}

fn frequency(state: &Channel, pitch: u8) -> Option<u16> {
    const NOTES: [i16; 25] = [
        0, -2004, -1891, -1785, -1685, -1590, -1501, -1417, -1337, -1262, -1192, -1125, -1062,
        -1002, -946, -893, -843, -795, -751, -709, -669, -631, -596, -563, -531,
    ];
    let index = usize::from(pitch + (state.transpose & 15));
    let value = *NOTES.get(index)?;
    let shift = 7u8.saturating_sub(state.octave + (state.transpose >> 4));
    Some((((value >> shift) as u16 & 0x7ff).wrapping_add(state.pitch_offset)) & 0x7ff)
}

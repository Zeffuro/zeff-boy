use std::collections::BTreeSet;

use super::{
    Budget, FREQUENCIES, FileSpan, LENGTHS, MAX_EVENTS, NesChannel, NesQueue, NesSection, NesSong,
    NesTermination, NesWarning, PROFILE, SONG_COUNT, ScanStop, TABLE, TITLES, pointer, span,
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
        ReadError::Invalid(reason) => anyhow::anyhow!("NES music validation failed: {reason}"),
        ReadError::Stop(stop) => anyhow::anyhow!("NES music validation stopped: {stop:?}"),
    }
}

pub struct Program {
    pub song: NesSong,
    pub notes: Vec<Note>,
    pub truncated: bool,
}

pub struct Note {
    pub channel: u8,
    pub frame: u32,
    pub duration: u32,
    pub timer: u16,
    pub drum: Option<u8>,
}

#[derive(Default)]
struct Channel {
    offset: u8,
    next_frame: u32,
    length: Option<u8>,
    events: u32,
    notes: u32,
}

struct Reader<'a, 'b, 'c> {
    bytes: &'a [u8],
    budget: &'b mut Budget<'c>,
    visited: BTreeSet<usize>,
    events: usize,
}

impl Reader<'_, '_, '_> {
    fn read(&mut self, address: u16) -> Result<u8, ReadError> {
        self.budget.charge()?;
        let offset = pointer(self.bytes, address, 1)?;
        self.visited.insert(offset);
        Ok(self.bytes[offset])
    }

    fn fetch(&mut self, base: u16, channel: &mut Channel) -> Result<(u16, u8), ReadError> {
        self.events += 1;
        if self.events > MAX_EVENTS {
            return Err(ScanStop::ValidationLimit.into());
        }
        let address = base
            .checked_add(u16::from(channel.offset))
            .ok_or_else(|| ReadError::Invalid("music data wraps PRG address space".into()))?;
        let value = self.read(address)?;
        channel.offset = channel.offset.wrapping_add(1);
        channel.events += 1;
        Ok((address, value))
    }

    fn length(&mut self, base: u8, adder: u8, index: u8) -> Result<u8, ReadError> {
        let index = u16::from(base) + u16::from(adder) + u16::from(index & 7);
        if index >= 48 {
            return Err(ReadError::Invalid(
                "note length index exceeds the qualified table".into(),
            ));
        }
        let value = self.read(LENGTHS + index)?;
        if value == 0 {
            return Err(ReadError::Invalid(
                "zero note length is not qualified".into(),
            ));
        }
        Ok(value)
    }

    fn frequency(&mut self, index: u8) -> Result<Option<u16>, ReadError> {
        if index & 1 != 0 || index >= 102 {
            return Err(ReadError::Invalid(
                "invalid frequency table byte offset".into(),
            ));
        }
        let low = self.read(FREQUENCIES + u16::from(index) + 1)?;
        if low == 0 {
            return Ok(None);
        }
        let high = self.read(FREQUENCIES + u16::from(index))?;
        Ok(Some((u16::from(high & 7) << 8) | u16::from(low)))
    }
}

struct Header {
    data: u16,
    length: u8,
    noise: u8,
    offsets: [u8; 4],
}

fn header(
    reader: &mut Reader<'_, '_, '_>,
    song: &mut NesSong,
    channels: &mut [Channel; 4],
    slot: u8,
    frame: u32,
) -> Result<Header, ReadError> {
    let table = TABLE + u16::from(slot);
    let address = TABLE + u16::from(reader.read(table)?);
    let mut raw = [0; 6];
    for (index, value) in raw.iter_mut().enumerate() {
        *value = reader.read(address + index as u16)?;
    }
    let header = Header {
        data: u16::from_le_bytes([raw[1], raw[2]]),
        length: raw[0],
        noise: raw[5],
        offsets: [0, raw[4], raw[3], raw[5]],
    };
    let section = NesSection {
        table_entry: span(pointer(reader.bytes, table, 1)?, 1),
        header: span(pointer(reader.bytes, address, 6)?, 6),
        cpu_address: header.data,
        start_frame: frame,
        end_frame: frame,
    };
    if song.sections.is_empty() {
        song.table_entry = section.table_entry;
        song.header = section.header;
        song.cpu_address = address;
        for number in 1..=4 {
            let index = match number {
                1 => 1,
                2 => 0,
                _ => number - 1,
            };
            let cpu_address = header
                .data
                .checked_add(u16::from(header.offsets[index as usize]))
                .ok_or_else(|| {
                    ReadError::Invalid("channel entry wraps PRG address space".into())
                })?;
            song.channels.push(NesChannel {
                number,
                entry: span(pointer(reader.bytes, cpu_address, 1)?, 1),
                cpu_address,
                event_count: 0,
                note_count: 0,
                end_frame: 0,
                termination: NesTermination::Unresolved,
                loop_start_frame: None,
            });
        }
    }
    song.sections.push(section);
    for (index, channel) in channels.iter_mut().enumerate() {
        channel.offset = header.offsets[index];
        channel.next_frame = if (index == 1 && channel.offset == 0)
            || (index == 3 && (song.queue != NesQueue::Area || song.selector & 0xf3 == 0))
        {
            u32::MAX
        } else {
            frame
        };
    }
    Ok(header)
}

pub(crate) fn interpret(
    bytes: &[u8],
    index: u8,
    loops: u8,
    max_frames: u32,
    budget: &mut Budget<'_>,
) -> Result<Program, ReadError> {
    if usize::from(index) >= SONG_COUNT || !(1..=8).contains(&loops) {
        return Err(ReadError::Invalid(
            "invalid song selector or pass count".into(),
        ));
    }
    let mut reader = Reader {
        bytes,
        budget,
        visited: BTreeSet::new(),
        events: 0,
    };
    let mut song = NesSong {
        profile: PROFILE,
        index,
        title: TITLES[usize::from(index)].into(),
        queue: if index < 8 {
            NesQueue::Event
        } else {
            NesQueue::Area
        },
        selector: 1 << (index & 7),
        table_entry: span(0, 0),
        header: span(0, 0),
        cpu_address: 0,
        channels: Vec::new(),
        sections: Vec::new(),
        mapped_spans: Vec::new(),
        warnings: Vec::new(),
        midi_exportable: false,
    };
    let ground = index == 8;
    let looping = index == 2 || (index >= 8 && song.selector & 0x5f != 0);
    let adder = if index == 6 { 8 } else { 0 };
    let mut slot = if ground { 16 } else { index };
    let mut channels = std::array::from_fn(|_| Channel::default());
    let mut current = header(&mut reader, &mut song, &mut channels, slot, 0)?;
    let mut notes = Vec::new();
    let mut section_note_start = 0;
    let mut completed_passes = 0;
    let mut truncated = false;
    let mut loop_start = if looping && !ground { Some(0) } else { None };
    let end_frame = loop {
        reader.budget.charge()?;
        // The driver services pulse 2 first; its terminator resets every channel on that frame.
        let channel_index = (0..4).min_by_key(|&i| (channels[i].next_frame, i)).unwrap();
        let frame = channels[channel_index].next_frame;
        if frame >= max_frames {
            truncated = true;
            break max_frames;
        }
        let (address, mut value) = reader.fetch(current.data, &mut channels[channel_index])?;
        if channel_index == 0 && value == 0 {
            close_section(&mut song, &mut notes, section_note_start, frame);
            if index == 6 {
                warning(
                    &mut song,
                    bytes,
                    address,
                    "Time Running Out restores prior area music; that runtime context is unavailable",
                )?;
            }
            if ground && slot < 48 {
                slot += 1;
                if slot == 17 {
                    loop_start = Some(frame);
                }
            } else {
                completed_passes += 1;
                if !looping || completed_passes == loops {
                    break frame;
                }
                if ground {
                    slot = 17;
                }
            }
            if song
                .sections
                .last()
                .is_some_and(|section| section.start_frame == frame)
            {
                return Err(ReadError::Invalid("zero-duration section cycle".into()));
            }
            section_note_start = notes.len();
            current = header(&mut reader, &mut song, &mut channels, slot, frame)?;
            continue;
        }
        if channel_index == 1 || channel_index == 3 {
            let mut zero_steps = 0;
            while value == 0 {
                zero_steps += 1;
                if zero_steps > 256 {
                    return Err(ReadError::Invalid(
                        "zero-duration channel control cycle".into(),
                    ));
                }
                if channel_index == 1 {
                    warning(
                        &mut song,
                        bytes,
                        address,
                        "pulse 1 hardware sweep requires APU synthesis and is not projected to MIDI",
                    )?;
                } else {
                    channels[channel_index].offset = current.noise;
                    if current.noise == 0 {
                        break;
                    }
                }
                value = reader.fetch(current.data, &mut channels[channel_index])?.1;
            }
            let length_index = ((value & 1) << 2) | (value >> 6);
            channels[channel_index].length =
                Some(reader.length(current.length, adder, length_index)?);
            value &= 0x3e;
        } else {
            if value & 0x80 != 0 {
                channels[channel_index].length =
                    Some(reader.length(current.length, adder, value)?);
                value = reader.fetch(current.data, &mut channels[channel_index])?.1;
            }
            if channel_index == 2 && value == 0 {
                // DEC of the un-reloaded zero triangle counter wraps and next fetches after 256 frames.
                channels[channel_index].next_frame = frame + 256;
                continue;
            }
        }
        let duration = u32::from(channels[channel_index].length.ok_or_else(|| {
            ReadError::Invalid("note uses an uninitialized runtime length buffer".into())
        })?);
        channels[channel_index].next_frame = if channel_index == 1 && channels[1].offset == 0 {
            u32::MAX
        } else {
            frame + duration
        };
        let drum = if channel_index == 3 {
            match value {
                0x30 => Some(46),
                0x20 => Some(38),
                v if v & 0x10 != 0 => Some(42),
                _ => None,
            }
        } else {
            None
        };
        let timer = if channel_index == 3 {
            drum.map(|_| 0)
        } else {
            reader.frequency(value)?
        };
        if let Some(timer) = timer {
            channels[channel_index].notes += 1;
            notes.push(Note {
                channel: [2, 1, 3, 4][channel_index],
                frame,
                duration,
                timer,
                drum,
            });
        }
    };
    close_section(&mut song, &mut notes, section_note_start, end_frame);
    for channel in &mut song.channels {
        let source = &channels[match channel.number {
            1 => 1,
            2 => 0,
            n => n - 1,
        } as usize];
        channel.event_count = source.events;
        channel.note_count = source.notes;
        channel.end_frame = end_frame;
        channel.loop_start_frame = loop_start;
        channel.termination = if !song.warnings.is_empty() || truncated {
            NesTermination::Unresolved
        } else if looping {
            NesTermination::Loop
        } else {
            NesTermination::Fine
        };
    }
    song.mapped_spans = spans(reader.visited);
    song.midi_exportable = song.warnings.is_empty() && !truncated;
    Ok(Program {
        song,
        notes,
        truncated,
    })
}

fn close_section(song: &mut NesSong, notes: &mut [Note], start: usize, frame: u32) {
    if let Some(section) = song.sections.last_mut() {
        section.end_frame = frame;
    }
    for note in &mut notes[start..] {
        note.duration = note.duration.min(frame.saturating_sub(note.frame));
    }
}

fn warning(song: &mut NesSong, bytes: &[u8], address: u16, reason: &str) -> Result<(), ReadError> {
    if !song.warnings.iter().any(|warning| warning.reason == reason) {
        song.warnings.push(NesWarning {
            offset: pointer(bytes, address, 1)? as u32,
            reason: reason.into(),
        });
    }
    Ok(())
}

fn spans(offsets: BTreeSet<usize>) -> Vec<FileSpan> {
    let mut result: Vec<FileSpan> = Vec::new();
    for offset in offsets {
        if let Some(last) = result.last_mut()
            && last.offset + last.byte_len == offset as u32
        {
            last.byte_len += 1;
        } else {
            result.push(span(offset, 1));
        }
    }
    result
}

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::{Budget, ScanLimits, ScanStop, tracker::FileSpan};

pub const SOURCE_REVISION: &str = "a3cbd0cea48e6784d7f625066d0300f7cb075926";

pub mod catalog;
pub mod discovery;
#[cfg(any(test, feature = "test-support"))]
pub mod fixture;
pub mod gbs;
pub mod isolation;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SongStructure {
    pub descriptor: FileSpan,
    pub ticks_per_row: u16,
    pub order_count: u8,
    pub loop_ticks: u32,
    pub notes: Vec<Note>,
    pub instruments: Vec<Instrument>,
    pub spans: Vec<FileSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Note {
    pub tick: u32,
    pub channel: u8,
    pub pitch: u8,
    pub instrument: u8,
    pub reload_instrument: bool,
    pub source: FileSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Instrument {
    pub channel: u8,
    pub id: u8,
    pub source: FileSpan,
    pub data: [u8; 6],
    pub wave: Option<FileSpan>,
}

pub fn inspect_plain_song(
    bytes: &[u8],
    descriptor: u16,
    limits: ScanLimits,
    cancel: &AtomicBool,
) -> Result<Option<SongStructure>, ScanStop> {
    if limits.max_work > crate::MAX_SCAN_WORK || limits.max_candidates > crate::MAX_CANDIDATES {
        return Err(ScanStop::InvalidLimits);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(ScanStop::Cancelled);
    }
    if bytes.len() != 0x8000 || bytes[0x147..0x14a] != [0, 0, 0] {
        return Ok(None);
    }
    if limits.max_candidates == 0 {
        return Err(ScanStop::CandidateLimit);
    }
    inspect_with_budget(
        bytes,
        descriptor,
        &mut Budget {
            cancel,
            remaining: limits.max_work,
        },
    )
}

fn inspect_with_budget(
    bytes: &[u8],
    descriptor: u16,
    budget: &mut Budget<'_>,
) -> Result<Option<SongStructure>, ScanStop> {
    let mut reader = Reader {
        bytes,
        budget,
        spans: BTreeMap::new(),
    };
    match inspect(&mut reader, usize::from(descriptor)) {
        Ok(song) => {
            reader.budget.charge()?;
            Ok(Some(song))
        }
        Err(Error::Invalid) => Ok(None),
        Err(Error::Stopped(stop)) => Err(stop),
    }
}

fn inspect(reader: &mut Reader<'_, '_, '_>, descriptor: usize) -> Result<SongStructure, Error> {
    let header = reader.read(descriptor, 21, Kind::Descriptor)?;
    let tempo = if header[0] == 0 {
        256
    } else {
        u16::from(header[0])
    };
    let count = reader.read(word(header, 1), 1, Kind::Count)?[0];
    if count == 0 || count % 2 != 0 || word(header, 17) != 0 {
        return Err(Error::Invalid);
    }
    let mut notes = Vec::new();
    let mut instruments = Vec::new();
    let mut used = BTreeSet::new();
    for channel in 0..4u8 {
        let orders = reader.read(
            word(header, 3 + usize::from(channel) * 2),
            usize::from(count),
            Kind::Orders,
        )?;
        let mut instrument = 0;
        let mut pitched = false;
        for order in 0..usize::from(count / 2) {
            let pointer = word(orders, order * 2);
            let pattern = reader.read(pointer, 192, Kind::Pattern)?;
            for row in 0..64 {
                reader.budget.charge()?;
                let cell = &pattern[row * 3..row * 3 + 3];
                if cell[1] & 15 != 0 || cell[2] != 0 || (cell[0] >= 72 && cell[0] != 90) {
                    return Err(Error::Invalid);
                }
                if cell[0] == 90 {
                    continue;
                }
                let next_instrument = cell[1] >> 4;
                if next_instrument != 0 {
                    instrument = next_instrument;
                    if used.insert((channel, instrument)) {
                        instruments.push(read_instrument(reader, header, channel, instrument)?);
                    }
                }
                if instrument == 0 {
                    return Err(Error::Invalid);
                }
                pitched = true;
                notes.push(Note {
                    tick: (order * 64 + row) as u32 * u32::from(tempo),
                    channel,
                    pitch: cell[0],
                    instrument,
                    reload_instrument: next_instrument != 0,
                    source: span(pointer + row * 3, 3),
                });
            }
        }
        if !pitched {
            return Err(Error::Invalid);
        }
    }
    notes.sort_by_key(|note| (note.tick, note.channel));
    Ok(SongStructure {
        descriptor: span(descriptor, 21),
        ticks_per_row: tempo,
        order_count: count / 2,
        loop_ticks: u32::from(count / 2) * 64 * u32::from(tempo),
        notes,
        instruments,
        spans: reader
            .spans
            .iter()
            .map(|(&at, &(len, _))| span(at, len))
            .collect(),
    })
}

fn read_instrument(
    reader: &mut Reader<'_, '_, '_>,
    header: &[u8],
    channel: u8,
    id: u8,
) -> Result<Instrument, Error> {
    let (field, kind, subpattern) = match channel {
        0 | 1 => (11, Kind::Duty, 3),
        2 => (13, Kind::WaveInstrument, 3),
        _ => (15, Kind::Noise, 1),
    };
    let pointer = word(header, field) + usize::from(id - 1) * 6;
    let data: [u8; 6] = reader.read(pointer, 6, kind)?.try_into().unwrap();
    if word(&data, subpattern) != 0 {
        return Err(Error::Invalid);
    }
    let wave = if channel == 2 {
        if data[2] > 15 {
            return Err(Error::Invalid);
        }
        let at = word(header, 19) + usize::from(data[2]) * 16;
        reader.read(at, 16, Kind::Wave)?;
        Some(span(at, 16))
    } else {
        None
    };
    Ok(Instrument {
        channel,
        id,
        source: span(pointer, 6),
        data,
        wave,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Descriptor,
    Count,
    Orders,
    Pattern,
    Duty,
    WaveInstrument,
    Noise,
    Wave,
}

struct Reader<'a, 'b, 'c> {
    bytes: &'a [u8],
    budget: &'b mut Budget<'c>,
    spans: BTreeMap<usize, (usize, Kind)>,
}

impl<'a> Reader<'a, '_, '_> {
    fn read(&mut self, at: usize, len: usize, kind: Kind) -> Result<&'a [u8], Error> {
        self.budget.charge()?;
        let end = at.checked_add(len).ok_or(Error::Invalid)?;
        let data = self.bytes.get(at..end).ok_or(Error::Invalid)?;
        if let Some((&previous, &(size, previous_kind))) = self.spans.range(..end).next_back()
            && previous + size > at
            && (previous != at || size != len || previous_kind != kind)
        {
            return Err(Error::Invalid);
        }
        self.spans.insert(at, (len, kind));
        Ok(data)
    }
}

enum Error {
    Invalid,
    Stopped(ScanStop),
}

impl From<ScanStop> for Error {
    fn from(stop: ScanStop) -> Self {
        Self::Stopped(stop)
    }
}

fn word(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

fn span(offset: usize, byte_len: usize) -> FileSpan {
    FileSpan {
        offset: offset as u32,
        byte_len: byte_len as u32,
    }
}

#[cfg(test)]
mod tests;

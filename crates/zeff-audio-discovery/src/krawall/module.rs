use std::collections::BTreeSet;

use super::{
    Budget, KrawallNativeProfile, KrawallSong, PatternEncoding, ReadError, RomSpan, ScanStop,
    banks, half, pointer, range,
};

const MAX_EVENTS: usize = 131_072;
const MAX_PATTERN_BYTES: usize = 131_072;

pub(super) struct Parsed {
    header: RomSpan,
    channels: u8,
    order_count: u8,
    pattern_count: u16,
    note_count: u32,
    instrument_count: u16,
    sample_count: u16,
    initial_speed: u8,
    initial_bpm: u8,
    mapped_spans: Vec<RomSpan>,
    pub subsongs: Vec<(u8, u8)>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Event {
    pub channel: u8,
    pub note: u8,
    pub instrument: u16,
    pub effect: u8,
}

impl Parsed {
    pub(super) fn song(
        &self,
        index: u16,
        subsong: u8,
        start_order: u8,
        native: &KrawallNativeProfile,
    ) -> KrawallSong {
        KrawallSong {
            profile: match native.encoding {
                PatternEncoding::Packed2003 => "gba-krawall-packed-2003",
                PatternEncoding::Extended2004 => "gba-krawall-extended-2004",
            },
            index,
            subsong,
            title: if self.subsongs.len() == 1 {
                format!("Krawall module at 0x{:08X}", self.header.canonical_cpu_address)
            } else {
                format!("Krawall module at 0x{:08X}, song {}",
                    self.header.canonical_cpu_address, subsong + 1)
            },
            header: self.header,
            channels: self.channels,
            start_order,
            order_count: self.order_count,
            pattern_count: self.pattern_count,
            note_count: self.note_count,
            instrument_count: self.instrument_count,
            sample_count: self.sample_count,
            initial_speed: self.initial_speed,
            initial_bpm: self.initial_bpm,
            native: native.clone(),
            mapped_spans: self.mapped_spans.clone(),
            warnings: vec![
                "Original Krawall driver playback repeats the selected order section until the requested duration; a natural ending is not inferred.".into(),
                "The inventory maps the module's patterns, possible note-map reads, samples and native setup witnesses. Unvisited instrument keys are preserved without repair.".into(),
                "Selectors sharing a starting order are deduplicated. Pattern control flow can connect order sections.".into(),
            ],
        }
    }
}

pub(super) fn looks_like_header(bytes: &[u8], offset: usize) -> bool {
    let Some(header) = bytes.get(offset..offset.saturating_add(368)) else {
        return false;
    };
    (1..=20).contains(&header[0])
        && header[1] != 0
        && header[2] < header[1]
        && header[3] < 254
        && header[3 + usize::from(header[2])] < 254
        && header[355] <= 128
        && (1..=16).contains(&header[356])
        && header[357] >= 30
        && header[358..363].iter().all(|&flag| flag <= 1)
        && header[363] == 0
}

pub(super) fn inspect(
    bytes: &[u8],
    offset: usize,
    native: &KrawallNativeProfile,
    budget: &mut Budget<'_>,
) -> Result<Parsed, ReadError> {
    budget.charge()?;
    if !offset.is_multiple_of(4) || !looks_like_header(bytes, offset) {
        return Err(ReadError::Invalid);
    }
    let fixed = range(bytes, offset, 364)?;
    let orders = &fixed[3..3 + usize::from(fixed[1])];
    if orders.contains(&255) {
        return Err(ReadError::Invalid);
    }
    let max_pattern = *orders
        .iter()
        .filter(|&&order| order < 254)
        .max()
        .ok_or(ReadError::Invalid)?;
    let header_len = 364 + (usize::from(max_pattern) + 1) * 4;
    range(bytes, offset, header_len)?;
    let mut events = Vec::new();
    let mut mapped_spans = vec![RomSpan::new(offset, header_len)];
    for index in 0..=max_pattern {
        budget.charge()?;
        let pattern = pointer(bytes, offset + 364 + usize::from(index) * 4, 34, 2)?;
        mapped_spans.push(read_pattern(
            bytes,
            pattern,
            fixed[0],
            native.encoding,
            &mut events,
            budget,
        )?);
    }
    let note_count = events
        .iter()
        .filter(|event| (1..=96).contains(&event.note))
        .count() as u32;
    let banks = match banks::inspect(bytes, native, fixed[358] != 0, &events, budget) {
        Err(ReadError::Invalid) => return Err(ReadError::UnsupportedGraph),
        result => result?,
    };
    let mut seen = BTreeSet::new();
    let subsongs = fixed[291..355]
        .iter()
        .enumerate()
        .filter_map(|(selector, &start)| {
            (usize::from(start) < orders.len()
                && orders[usize::from(start)] < 254
                && seen.insert(start))
            .then_some((selector as u8, start))
        })
        .collect::<Vec<_>>();
    if subsongs.is_empty() {
        return Err(ReadError::Invalid);
    }
    mapped_spans.extend(banks.spans);
    mapped_spans.push(native.process_row);
    mapped_spans.extend(&native.setup_spans);
    mapped_spans.extend(native.ram_copies.iter().map(|copy| copy.source));
    super::merge_spans(&mut mapped_spans);
    Ok(Parsed {
        header: RomSpan::new(offset, header_len),
        channels: fixed[0],
        order_count: fixed[1],
        pattern_count: u16::from(max_pattern) + 1,
        note_count,
        instrument_count: banks.instrument_count,
        sample_count: banks.sample_count,
        initial_speed: fixed[356],
        initial_bpm: fixed[357],
        mapped_spans,
        subsongs,
    })
}

fn read_pattern(
    bytes: &[u8],
    offset: usize,
    channels: u8,
    encoding: PatternEncoding,
    events: &mut Vec<Event>,
    budget: &mut Budget<'_>,
) -> Result<RomSpan, ReadError> {
    range(bytes, offset, 34)?;
    let extended = encoding == PatternEncoding::Extended2004;
    let rows = if extended {
        half(bytes, offset + 32).unwrap()
    } else {
        u16::from(bytes[offset + 32])
    };
    if !(1..=256).contains(&rows) {
        return Err(ReadError::Invalid);
    }
    let begin = offset + if extended { 34 } else { 33 };
    let mut cursor = begin;
    for row in 0..rows {
        budget.charge()?;
        if row < 64
            && row % 4 == 0
            && usize::from(half(bytes, offset + usize::from(row / 4) * 2).unwrap())
                != cursor - begin
        {
            return Err(ReadError::Invalid);
        }
        let mut terminated = false;
        for _ in 0..=128 {
            budget.charge()?;
            if cursor - begin >= MAX_PATTERN_BYTES {
                return Err(ReadError::Stop(ScanStop::ValidationLimit));
            }
            let follow = *bytes.get(cursor).ok_or(ReadError::Invalid)?;
            cursor += 1;
            if follow == 0 {
                terminated = true;
                break;
            }
            if follow & 31 >= channels || follow & 0xe0 == 0 {
                return Err(ReadError::Invalid);
            }
            let mut event = Event {
                channel: follow & 31,
                note: 0,
                instrument: 0,
                effect: 0,
            };
            if follow & 32 != 0 {
                let pair = range(bytes, cursor, 2)?;
                event.note = pair[0];
                event.instrument = u16::from(pair[1]);
                cursor += 2;
                if extended && event.note & 128 != 0 {
                    event.instrument |=
                        u16::from(*bytes.get(cursor).ok_or(ReadError::Invalid)?) << 8;
                    event.note &= 127;
                    cursor += 1;
                } else if !extended {
                    event.instrument |= u16::from(event.note & 1) << 8;
                    event.note >>= 1;
                }
                if event.note > 96 && event.note != 127 {
                    return Err(ReadError::Invalid);
                }
            }
            if follow & 64 != 0 {
                range(bytes, cursor, 1)?;
                cursor += 1;
            }
            if follow & 128 != 0 {
                event.effect = range(bytes, cursor, 2)?[0];
                cursor += 2;
                if event.effect > 50 {
                    return Err(ReadError::Invalid);
                }
            }
            if events.len() >= MAX_EVENTS {
                return Err(ReadError::Stop(ScanStop::ValidationLimit));
            }
            events.push(event);
        }
        if !terminated {
            return Err(ReadError::Stop(ScanStop::ValidationLimit));
        }
    }
    Ok(RomSpan::new(offset, cursor - offset))
}

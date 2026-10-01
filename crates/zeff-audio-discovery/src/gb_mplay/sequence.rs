use std::collections::BTreeSet;

use crate::Budget;

use super::{
    GbMplaySong, GbMplayTrack, ReadError,
    profiles::{OrderLayout, Profile, Recognition},
    span,
};

#[path = "instruments.rs"]
mod instruments;

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    profile: &'static Profile,
    banks: BTreeSet<u16>,
}

impl Reader<'_> {
    fn byte(&mut self, bank: u16, address: u16) -> Result<u8, ReadError> {
        if bank == 0 || bank > 255 || !(0x4000..0x8000).contains(&address) {
            return Err(ReadError::Invalid);
        }
        let offset = usize::from(bank) * 0x4000 + usize::from(address - 0x4000);
        let value = self.bytes.get(offset).copied().ok_or(ReadError::Invalid)?;
        self.banks.insert(bank);
        Ok(value)
    }

    fn word(&mut self, address: u16) -> Result<u16, ReadError> {
        Ok(u16::from_le_bytes([
            self.byte(self.profile.bank, address)?,
            self.byte(
                self.profile.bank,
                address.checked_add(1).ok_or(ReadError::Invalid)?,
            )?,
        ]))
    }

    fn owned(&mut self, address: u16, start: u16, end: u16) -> Result<u8, ReadError> {
        if !(start..end).contains(&address) {
            return Err(ReadError::Invalid);
        }
        self.byte(self.profile.bank, address)
    }
}

struct Tables {
    orders: Vec<u16>,
    groups: Vec<u16>,
    banks: Vec<u16>,
    order_starts: Vec<u16>,
    order_ends: Vec<u16>,
    group_ends: Vec<u16>,
    bank_ends: Vec<u16>,
}

impl Tables {
    fn read(reader: &mut Reader<'_>, budget: &mut Budget<'_>) -> Result<Self, ReadError> {
        let p = reader.profile;
        let mut tables = Self {
            orders: Vec::new(),
            groups: Vec::new(),
            banks: Vec::new(),
            order_starts: Vec::new(),
            order_ends: Vec::new(),
            group_ends: Vec::new(),
            bank_ends: Vec::new(),
        };
        for index in 0..p.selector_count {
            budget.charge()?;
            let (order, groups, banks) = match p.layout {
                OrderLayout::Indexed { groups, banks, .. } => (p.order, groups, banks),
                OrderLayout::Indirect { selectors } => (
                    reader.word(selectors[0] + index * 2)?,
                    reader.word(selectors[1] + index * 2)?,
                    reader.word(selectors[2] + index * 2)?,
                ),
            };
            if !(p.order..p.order_end).contains(&order)
                || !(0x4000..0x8000).contains(&groups)
                || !(groups..0x8000).contains(&banks)
            {
                return Err(ReadError::Invalid);
            }
            let start = match p.layout {
                OrderLayout::Indexed { selectors, .. } => order
                    .checked_add(u16::from(reader.byte(p.bank, selectors + index)?))
                    .ok_or(ReadError::Invalid)?,
                OrderLayout::Indirect { .. } => order,
            };
            if !(p.order..p.order_end).contains(&start) {
                return Err(ReadError::Invalid);
            }
            tables.order_starts.push(start);
            tables.orders.push(order);
            tables.groups.push(groups);
            tables.banks.push(banks);
        }
        for &start in &tables.order_starts {
            tables.order_ends.push(
                tables
                    .order_starts
                    .iter()
                    .copied()
                    .filter(|&v| v > start)
                    .min()
                    .unwrap_or(p.order_end),
            );
        }
        for (&groups, &banks) in tables.groups.iter().zip(&tables.banks) {
            let end = tables
                .groups
                .iter()
                .chain(&tables.banks)
                .copied()
                .filter(|&v| v > groups)
                .min()
                .ok_or(ReadError::Invalid)?;
            if !(end - groups).is_multiple_of(8) {
                return Err(ReadError::Invalid);
            }
            let bank_end = banks
                .checked_add((end - groups) / 2)
                .ok_or(ReadError::Invalid)?;
            if bank_end > 0x8000
                || tables
                    .banks
                    .iter()
                    .any(|&next| next > banks && next < bank_end)
            {
                return Err(ReadError::Invalid);
            }
            tables.group_ends.push(end);
            tables.bank_ends.push(bank_end);
        }
        Ok(tables)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Channel {
    pointer: u16,
    bank: u16,
    duration: u8,
    effect: u8,
    parameter: u8,
    pending: bool,
    instrument: Option<u8>,
    active: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct State {
    order: u8,
    new_group: bool,
    speed: u8,
    channels: [Channel; 4],
}

#[derive(Default)]
struct Usage {
    notes: [BTreeSet<u8>; 4],
    instruments: [BTreeSet<u8>; 4],
    arpeggios: [BTreeSet<u8>; 4],
    counts: [u32; 4],
}

fn group(
    reader: &mut Reader<'_>,
    tables: &Tables,
    index: usize,
    state: &mut State,
    initial: u8,
) -> Result<bool, ReadError> {
    let p = reader.profile;
    let order = tables.orders[index]
        .checked_add(u16::from(state.order))
        .ok_or(ReadError::Invalid)?;
    let group = reader.owned(order, tables.order_starts[index], tables.order_ends[index])?;
    state.order = state.order.wrapping_add(1);
    if group == 254 {
        return Ok(false);
    }
    if group == 255 {
        return Err(ReadError::Invalid);
    }
    let next = order.checked_add(1).ok_or(ReadError::Invalid)?;
    if reader.owned(next, tables.order_starts[index], tables.order_ends[index])? == 255 {
        state.order = initial;
    }
    let gp = tables.groups[index]
        .checked_add(u16::from(group) * 8)
        .ok_or(ReadError::Invalid)?;
    let bp = tables.banks[index]
        .checked_add(u16::from(group) * 4)
        .ok_or(ReadError::Invalid)?;
    if u32::from(gp) + 8 > u32::from(tables.group_ends[index])
        || u32::from(bp) + 4 > u32::from(tables.bank_ends[index])
    {
        return Err(ReadError::Invalid);
    }
    for (number, channel) in state.channels.iter_mut().enumerate() {
        channel.pointer = reader.word(gp + number as u16 * 2)?;
        channel.bank = p.bank + u16::from(reader.byte(p.bank, bp + number as u16)?);
        if channel.bank > 255 {
            return Err(ReadError::Invalid);
        }
        channel.duration = 0;
    }
    state.new_group = false;
    Ok(true)
}

fn event(
    reader: &mut Reader<'_>,
    channel: &mut Channel,
    number: usize,
    usage: &mut Usage,
) -> Result<bool, ReadError> {
    if channel.duration > 0 {
        channel.duration -= 1;
        return Ok(false);
    }
    let mut get = || {
        if channel.bank == reader.profile.bank && channel.pointer < reader.profile.tone {
            return Err(ReadError::Invalid);
        }
        let value = reader.byte(channel.bank, channel.pointer)?;
        channel.pointer = channel.pointer.checked_add(1).ok_or(ReadError::Invalid)?;
        Ok::<_, ReadError>(value)
    };
    let flags = get()?;
    if flags & 128 != 0 || (flags & 64 == 0 && flags & 15 != 0) {
        return Err(ReadError::Invalid);
    }
    let note = if flags & 16 != 0 { Some(get()?) } else { None };
    if flags & 32 != 0 {
        let instrument = get()?;
        channel.instrument = Some(instrument);
        usage.instruments[number].insert(instrument);
    }
    if flags & 64 != 0 {
        channel.effect = flags & 15;
        channel.parameter = get()?;
        if !matches!(channel.effect, 0..=4 | 11 | 12 | 15) {
            return Err(ReadError::Invalid);
        }
        if channel.effect == 0 && channel.parameter != 0 {
            usage.arpeggios[number].insert(channel.parameter);
        }
    }
    if channel.active && flags & 32 != 0 && (note.is_none() || channel.effect == 3) {
        return Err(ReadError::Invalid);
    }
    channel.pending = flags & 80 != 0;
    channel.duration = get()?;
    if let Some(note) = note {
        channel.active = true;
        usage.instruments[number].insert(channel.instrument.ok_or(ReadError::Invalid)?);
        usage.notes[number].insert(note);
        usage.counts[number] += 1;
        if number < 3 {
            if note & 127 >= 72 {
                return Err(ReadError::Invalid);
            }
            reader.word(reader.profile.tone + 2 * u16::from(note & 127))?;
        }
    }
    Ok(number == 0 && reader.byte(channel.bank, channel.pointer)? == 255)
}

pub(super) fn song(
    bytes: &[u8],
    recognized: Recognition,
    index: u16,
    budget: &mut Budget<'_>,
) -> Result<GbMplaySong, ReadError> {
    let p = recognized.profile;
    if index >= p.selector_count {
        return Err(ReadError::Invalid);
    }
    let mut reader = Reader {
        bytes,
        profile: p,
        banks: BTreeSet::new(),
    };
    let tables = Tables::read(&mut reader, budget)?;
    let (initial, speed, entry, entry_len) = match p.layout {
        OrderLayout::Indexed {
            selectors, speed, ..
        } => (
            reader.byte(p.bank, selectors + index)?,
            reader.byte(p.bank, speed)?,
            selectors + index,
            1,
        ),
        OrderLayout::Indirect { selectors } => (0, 2, selectors[0] + index * 2, 2),
    };
    if speed == 0 || speed == 255 {
        return Err(ReadError::Invalid);
    }
    let mut state = State {
        order: initial,
        new_group: true,
        speed,
        channels: [Channel::default(); 4],
    };
    let mut seen = BTreeSet::new();
    let mut usage = Usage::default();
    let mut complete = false;
    for _ in 0..100_000 {
        budget.charge()?;
        if !seen.insert(state) {
            complete = true;
            break;
        }
        for channel in &mut state.channels {
            if channel.pending {
                if channel.effect == 11 {
                    state.order = channel.parameter;
                }
                if channel.effect == 15 {
                    if !(2..255).contains(&channel.parameter) {
                        return Err(ReadError::Invalid);
                    }
                    state.speed = channel.parameter;
                    channel.effect = 0;
                    channel.parameter = 0;
                }
                channel.pending = false;
            }
        }
        if state.new_group
            && state.channels[0].duration == 0
            && !group(
                &mut reader,
                &tables,
                usize::from(index),
                &mut state,
                initial,
            )?
        {
            complete = true;
            break;
        }
        for (number, channel) in state.channels.iter_mut().enumerate() {
            state.new_group |= event(&mut reader, channel, number, &mut usage)?;
        }
    }
    if !complete || usage.counts.iter().all(|&count| count == 0) {
        return Err(ReadError::Invalid);
    }
    for number in 0..4 {
        for &instrument in &usage.instruments[number] {
            instruments::validate(
                &mut reader,
                number,
                instrument,
                &usage.notes[number],
                &usage.arpeggios[number],
                budget,
            )?;
        }
    }
    let mut mapped_spans = vec![span(0, 0, 0x4000)];
    mapped_spans.extend(
        reader
            .banks
            .into_iter()
            .map(|bank| span(bank, 0x4000, 0x4000)),
    );
    Ok(GbMplaySong {
        profile: p.name,
        index,
        title: format!("Audio selection {index}"),
        bank: p.bank,
        order_address: tables.orders[usize::from(index)] + u16::from(initial),
        table_entry: span(p.bank, entry, entry_len),
        tracks: usage.counts.into_iter().enumerate().map(|(i, note_count)| GbMplayTrack {
            number: i as u8 + 1,
            note_count,
        }).collect(),
        mapped_spans,
        warnings: vec!["Native CGB double-speed driver, called once per VBlank. Generated waves and unsupported sequence commands are excluded; audio role, duration and complete soundtrack membership are unknown.".into()],
    })
}

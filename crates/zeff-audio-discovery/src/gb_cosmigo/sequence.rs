use std::{collections::BTreeSet, ops::Range};

use crate::Budget;

use super::{GbCosmigoSong, GbCosmigoTrack, ReadError, profiles::Recognition, span};

struct Module<'a> {
    data: &'a [u8],
    songs: u16,
    patterns: Vec<u16>,
    orders: Range<u16>,
    pattern_data: Range<u16>,
    instruments: Range<u16>,
    envelopes: Range<u16>,
    waves: Range<u16>,
}

fn invalid<T>() -> Result<T, ReadError> {
    Err(ReadError::Invalid)
}

impl<'a> Module<'a> {
    fn new(data: &'a [u8]) -> Result<Self, ReadError> {
        let ptr = |offset| u16::from_le_bytes([data[offset], data[offset + 1]]);
        let songs = ptr(12);
        let patterns = ptr(14);
        let instruments = ptr(16);
        let envelopes = ptr(18);
        let waves = ptr(20);
        if !(0x5000 < instruments
            && instruments < envelopes
            && envelopes < waves
            && waves.checked_add(64) == Some(patterns)
            && patterns < songs
            && songs <= 0x7ffb
            && (envelopes - instruments).is_multiple_of(2)
            && (songs - patterns).is_multiple_of(2))
        {
            return invalid();
        }
        let patterns: Vec<_> = (patterns..songs)
            .step_by(2)
            .map(|address| ptr(usize::from(address - 0x4000)))
            .collect();
        let first = patterns.iter().copied().min().ok_or(ReadError::Invalid)?;
        if first <= 0x5000 || patterns.iter().any(|&p| p >= instruments) {
            return invalid();
        }
        Ok(Self {
            data,
            songs,
            patterns,
            orders: 0x5000..first,
            pattern_data: first..instruments,
            instruments: instruments..envelopes,
            envelopes: envelopes..waves,
            waves: waves..waves + 64,
        })
    }

    fn read<const N: usize>(&self, at: u16, region: &Range<u16>) -> Result<[u8; N], ReadError> {
        if at < region.start || usize::from(at) + N > usize::from(region.end) {
            return invalid();
        }
        self.data[usize::from(at - 0x4000)..][..N]
            .try_into()
            .map_err(|_| ReadError::Invalid)
    }

    fn envelope_start(&self, instrument: u8) -> Result<u16, ReadError> {
        if instrument >= 128 {
            return invalid();
        }
        let [high, low] = self.read(
            self.instruments.start + u16::from(instrument) * 2,
            &self.instruments,
        )?;
        self.envelopes
            .start
            .checked_add(u16::from_be_bytes([high & 63, low]))
            .ok_or(ReadError::Invalid)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Channel {
    instrument: u8,
    note: u8,
    transpose: u8,
    envelope: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct State {
    order: u16,
    row: u8,
    timer: u8,
    channels: [Channel; 4],
}

fn row(
    module: &Module<'_>,
    state: &mut State,
    start: u16,
    notes: &mut [u32; 4],
    budget: &mut Budget<'_>,
) -> Result<Option<[bool; 4]>, ReadError> {
    let mut immediate = BTreeSet::new();
    let mut restart = [false; 4];
    loop {
        budget.charge()?;
        if !immediate.insert((state.order, state.row)) {
            return invalid();
        }
        match module.read::<1>(state.order, &module.orders)?[0] {
            254 => return Ok(None),
            255 => {
                let offset = u16::from_le_bytes(module.read(state.order + 1, &module.orders)?);
                state.order = start.checked_add(offset).ok_or(ReadError::Invalid)?;
                continue;
            }
            _ => (),
        }
        let order = module.read::<7>(state.order, &module.orders)?;
        let mut advance = false;
        for (number, channel) in state.channels.iter_mut().enumerate() {
            let pattern = order[if number < 3 { number * 2 } else { 6 }];
            if number < 3 {
                channel.transpose = order[number * 2 + 1];
            }
            if pattern == 253 {
                continue;
            }
            let address = module
                .patterns
                .get(usize::from(pattern))
                .ok_or(ReadError::Invalid)?
                .checked_add(u16::from(state.row) * 3)
                .ok_or(ReadError::Invalid)?;
            let [instrument, _effect, note] = module.read(address, &module.pattern_data)?;
            if instrument != 0 {
                channel.instrument = instrument - 1;
                module.envelope_start(channel.instrument)?;
            }
            if number == 0 && note == 255 {
                state.order = state.order.checked_add(7).ok_or(ReadError::Invalid)?;
                state.row = 0;
                advance = true;
                break;
            }
            if matches!(note, 253 | 254) {
                continue;
            }
            channel.note = note & 127;
            if note < 128 {
                restart[number] = true;
                notes[number] += 1;
            }
        }
        if !advance {
            return Ok(Some(restart));
        }
    }
}

fn envelopes(module: &Module<'_>, state: &mut State, restart: [bool; 4]) -> Result<(), ReadError> {
    for (number, channel) in state.channels.iter_mut().enumerate() {
        let at = if restart[number] {
            let at = module.envelope_start(channel.instrument)?;
            if number == 2 {
                let waveform = module.read::<1>(at, &module.envelopes)?[0] & 15;
                module.read::<16>(module.waves.start + u16::from(waveform) * 16, &module.waves)?;
            }
            at
        } else {
            match module.read::<1>(channel.envelope, &module.envelopes)?[0] {
                254 => continue,
                255 => {
                    let offset =
                        u16::from_le_bytes(module.read(channel.envelope + 1, &module.envelopes)?);
                    module
                        .envelopes
                        .start
                        .checked_add(offset)
                        .ok_or(ReadError::Invalid)?
                }
                _ => channel.envelope,
            }
        };
        let [volume, delta, flags] = module.read(at, &module.envelopes)?;
        // Restart and jump targets bypass the driver's sentinel check.
        if volume >= 254 {
            return invalid();
        }
        channel.envelope = at + 3;
        let base = if number < 3 && flags & 1 != 0 {
            0
        } else {
            channel
                .note
                .wrapping_add(if number < 3 { channel.transpose } else { 0 })
        };
        if base.wrapping_add(delta) >= if number == 3 { 8 } else { 84 } {
            return invalid();
        }
    }
    Ok(())
}

pub(super) fn song(
    bytes: &[u8],
    recognized: Recognition,
    index: u16,
    budget: &mut Budget<'_>,
) -> Result<GbCosmigoSong, ReadError> {
    budget.charge()?;
    if index > 255 {
        return invalid();
    }
    let bank = recognized.bank;
    let offset = usize::from(bank) * 0x4000;
    let module = Module::new(&bytes[offset..offset + 0x4000])?;
    let entry = module.songs + index * 5;
    let [lo, hi, length, speed, volume] = module.read(entry, &(module.songs..0x8000))?;
    if length == 0 || speed == 0 || volume > 7 {
        return invalid();
    }
    let start = u16::from_le_bytes([lo, hi]);
    let mut state = State {
        order: start,
        row: 0,
        timer: 1,
        channels: [Channel::default(); 4],
    };
    let mut seen = BTreeSet::new();
    let mut notes = [0; 4];
    let mut complete = false;
    for _ in 0..120_000 {
        budget.charge()?;
        if !seen.insert(state) {
            complete = true;
            break;
        }
        state.timer -= 1;
        let mut restart = [false; 4];
        if state.timer == 0 {
            let Some(next) = row(&module, &mut state, start, &mut notes, budget)? else {
                complete = true;
                break;
            };
            restart = next;
            state.timer = speed;
            state.row = state.row.wrapping_add(1);
            if state.row == length {
                state.order = state.order.checked_add(7).ok_or(ReadError::Invalid)?;
                state.row = 0;
            }
        }
        envelopes(&module, &mut state, restart)?;
    }
    if !complete || notes.iter().all(|&n| n == 0) {
        return invalid();
    }
    Ok(GbCosmigoSong {
        profile: recognized.profile.name,
        index,
        title: format!("Bank {bank:02X} audio selection {index}"),
        bank,
        order_address: start,
        table_entry: span(bank, entry, 5),
        tracks: notes.into_iter().enumerate().map(|(i, note_count)| GbCosmigoTrack { number: i as u8 + 1, note_count }).collect(),
        mapped_spans: vec![span(bank, 0x4000, 0x4000)],
        warnings: vec!["Native CGB double-speed driver, called once per VBlank. Only selections with bank-contained sequence and envelope reads are included; audio role, duration and complete soundtrack membership are unknown.".into()],
    })
}

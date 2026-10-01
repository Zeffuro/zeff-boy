use std::collections::BTreeSet;

use crate::Budget;

use super::{GbImedSong, GbImedTrack, ReadError, profiles::Recognition, span};

type Row = [[u8; 4]; 4];

struct Module<'a> {
    bytes: &'a [u8],
    bank: u16,
    start: u16,
    end: u16,
}

impl Module<'_> {
    fn byte(&self, offset: u16) -> Result<u8, ReadError> {
        let address = self.start.checked_add(offset).ok_or(ReadError::Invalid)?;
        if address >= self.end {
            return Err(ReadError::Invalid);
        }
        self.bytes
            .get(usize::from(self.bank) * 0x4000 + usize::from(address - 0x4000))
            .copied()
            .ok_or(ReadError::Invalid)
    }

    fn word(&self, offset: u16) -> Result<u16, ReadError> {
        Ok(u16::from_le_bytes([
            self.byte(offset)?,
            self.byte(offset.checked_add(1).ok_or(ReadError::Invalid)?)?,
        ]))
    }
}

struct Patterns {
    orders: Vec<u8>,
    rows: Vec<Vec<Row>>,
    pulse: u16,
    wave_patch: u16,
    waves: u16,
    noise: u16,
}

impl Patterns {
    fn read(module: &Module<'_>, budget: &mut Budget<'_>) -> Result<Self, ReadError> {
        for (i, byte) in b"IMEDGBoy".iter().enumerate() {
            if module.byte(i as u16)? != *byte {
                return Err(ReadError::Invalid);
            }
        }
        let list = module.word(8)?;
        let directory = module.word(10)?;
        let pulse = module.word(12)?;
        let wave_patch = module.word(14)?;
        let waves = module.word(16)?;
        let noise = module.word(18)?;
        if list != 20
            || directory < list + 4
            || directory - list > 131
            || directory >= pulse
            || pulse > wave_patch
            || wave_patch > waves
            || u32::from(waves) + 16 > u32::from(noise)
            || !(wave_patch - pulse).is_multiple_of(4)
            || !(waves - wave_patch).is_multiple_of(4)
            || module.byte(directory - 3)? != 255
            || module.byte(directory - 1)? != 0
        {
            return Err(ReadError::Invalid);
        }
        let count = module.byte(directory - 2)?;
        if count == 0 || count > 128 {
            return Err(ReadError::Invalid);
        }
        let mut orders = Vec::new();
        for offset in list..directory - 3 {
            budget.charge()?;
            let index = module.byte(offset)?;
            if index >= count || index >= 128 {
                return Err(ReadError::Invalid);
            }
            orders.push(index);
        }
        let mut pointers = Vec::new();
        for index in 0..=u16::from(count) {
            let offset = directory.checked_add(index * 2).ok_or(ReadError::Invalid)?;
            pointers.push(
                directory
                    .checked_add(module.word(offset)?)
                    .ok_or(ReadError::Invalid)?,
            );
        }
        if pointers[0] != directory + (u16::from(count) + 1) * 2
            || pointers.last().copied() != Some(pulse)
            || pointers.windows(2).any(|p| p[0] >= p[1])
        {
            return Err(ReadError::Invalid);
        }
        let mut patterns = Vec::new();
        for bounds in pointers.windows(2) {
            let mut cursor = bounds[0];
            let mut rows = Vec::new();
            for _ in 0..64 {
                budget.charge()?;
                let mut get = || {
                    if cursor >= bounds[1] {
                        return Err(ReadError::Invalid);
                    }
                    let byte = module.byte(cursor)?;
                    cursor += 1;
                    Ok(byte)
                };
                let mask = u16::from_be_bytes([get()?, get()?]);
                let mut row = [[0; 4]; 4];
                for (i, byte) in row.iter_mut().flatten().enumerate() {
                    if mask & (0x8000 >> i) != 0 {
                        *byte = get()?;
                    }
                }
                rows.push(row);
            }
            if cursor != bounds[1] {
                return Err(ReadError::Invalid);
            }
            patterns.push(rows);
        }
        Ok(Self {
            orders,
            rows: patterns,
            pulse,
            wave_patch,
            waves,
            noise,
        })
    }
}

#[derive(Default)]
struct Usage {
    notes: [BTreeSet<u8>; 4],
    patches: [BTreeSet<u8>; 4],
    arps: [BTreeSet<u8>; 3],
    counts: [u32; 4],
}

fn traverse(patterns: &Patterns, budget: &mut Budget<'_>) -> Result<Usage, ReadError> {
    let mut usage = Usage::default();
    let mut current = (0usize, 0usize);
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        budget.charge()?;
        let (order, row_index) = current;
        let index = *patterns.orders.get(order).ok_or(ReadError::Invalid)?;
        let row = &patterns.rows[usize::from(index)][row_index];
        let mut jump = None;
        let mut next = row_index == 63;
        for (channel, &[note, patch, effect, parameter]) in row.iter().enumerate() {
            if (channel < 3 && (note > 96 || effect > 15))
                || (channel == 3 && patch != 0 && note >= 112)
            {
                return Err(ReadError::Invalid);
            }
            if note != 0 {
                usage.notes[channel].insert(note);
                usage.counts[channel] += 1;
            }
            if patch != 0 {
                usage.patches[channel].insert(patch);
            }
            if channel < 3 {
                if effect == 0 && parameter != 0 {
                    usage.arps[channel].insert(parameter >> 4);
                    usage.arps[channel].insert(parameter & 15);
                }
                if effect == 11 {
                    if usize::from(parameter) >= patterns.orders.len() || parameter >= 128 {
                        return Err(ReadError::Invalid);
                    }
                    next = true;
                    jump.get_or_insert(usize::from(parameter));
                }
                next |= effect == 13;
            }
        }
        current = if next {
            (jump.unwrap_or((order + 1) % patterns.orders.len()), 0)
        } else {
            (order, row_index + 1)
        };
    }
    if usage.counts.iter().all(|&n| n == 0) {
        return Err(ReadError::Invalid);
    }
    Ok(usage)
}

fn instruments(module: &Module<'_>, patterns: &Patterns, usage: &Usage) -> Result<(), ReadError> {
    let mut audible = false;
    for i in 0..16 {
        module.byte(patterns.waves + i)?;
    }
    for channel in 0..4 {
        let (start, end, stride, maximum) = match channel {
            0 | 1 => (patterns.pulse, patterns.wave_patch, 4, 64),
            2 => (patterns.wave_patch, patterns.waves, 4, 64),
            _ => (patterns.noise, module.end - module.start, 2, 128),
        };
        for &patch in &usage.patches[channel] {
            let address = start
                .checked_add(u16::from(patch - 1) * stride)
                .ok_or(ReadError::Invalid)?;
            if patch > maximum || u32::from(address) + u32::from(stride) > u32::from(end) {
                return Err(ReadError::Invalid);
            }
            for i in 0..stride {
                module.byte(address + i)?;
            }
            if usage.counts[channel] > 0 {
                audible |= match channel {
                    0 | 1 => module.byte(address + 2)? & 0xf8 != 0,
                    2 => module.byte(address + 1)? & 0x60 != 0,
                    _ => module.byte(address + 1)? & 0xf8 != 0,
                };
            }
            if channel == 2 {
                let wave = module.byte(address + 2)?;
                let start = u32::from(patterns.waves) + u32::from(wave) * 16;
                if start + 16 > u32::from(patterns.noise) {
                    return Err(ReadError::Invalid);
                }
                for i in 0..16 {
                    module.byte(start as u16 + i)?;
                }
            }
        }
        if channel < 3 {
            for offset in &usage.arps[channel] {
                if usage.notes[channel]
                    .iter()
                    .chain(&[1])
                    .any(|&note| u16::from(note) + u16::from(*offset) > 96)
                {
                    return Err(ReadError::Invalid);
                }
            }
        }
    }
    if !audible {
        return Err(ReadError::Invalid);
    }
    Ok(())
}

pub(super) fn song(
    bytes: &[u8],
    recognized: Recognition,
    index: u16,
    address: u16,
    budget: &mut Budget<'_>,
) -> Result<GbImedSong, ReadError> {
    let bank = recognized.bank;
    if !(0x4000..0x8000).contains(&address) {
        return Err(ReadError::Invalid);
    }
    let base = usize::from(bank) * 0x4000;
    let window = bytes.get(base..base + 0x4000).ok_or(ReadError::Invalid)?;
    let after = usize::from(address - 0x4000) + 8;
    let end = window
        .get(after..)
        .ok_or(ReadError::Invalid)?
        .windows(8)
        .position(|s| s == b"IMEDGBoy")
        .map_or(0x8000, |offset| (0x4000 + after + offset) as u16);
    let module = Module {
        bytes,
        bank,
        start: address,
        end,
    };
    let patterns = Patterns::read(&module, budget)?;
    let usage = traverse(&patterns, budget)?;
    instruments(&module, &patterns, &usage)?;
    let mut banks = BTreeSet::from([0, bank]);
    banks.extend(recognized.profile.extra_banks);
    Ok(GbImedSong {
        profile: recognized.profile.name,
        index,
        title: format!("Audio selection {index}"),
        bank,
        module_address: address,
        table_entry: span(bank, address, 20),
        tracks: usage.counts.into_iter().enumerate().map(|(i, note_count)| GbImedTrack {
            number: i as u8 + 1,
            note_count,
        }).collect(),
        mapped_spans: banks.into_iter().map(|b| {
            span(b, if b == 0 { 0 } else { 0x4000 }, 0x4000)
        }).collect(),
        warnings: vec!["Native IMEDGBoy driver with original playback cadence. Compressed modules and unsafe references are excluded; audio role, duration and complete soundtrack membership are unknown.".into()],
    })
}

use std::collections::HashSet;

use crate::{Budget, RomSpan};

use super::{ReadError, profiles::Profile};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct State {
    index: u16,
    repeats: [(u8, u16); 4],
    call_count: u8,
    return_index: u16,
    variable: u8,
    volume: u8,
    volume_rise: bool,
}

pub(super) struct Reader<'a, 'b, 'c> {
    bytes: &'a [u8],
    bank: usize,
    profile: Profile,
    legacy: bool,
    cpu_base: u32,
    budget: &'b mut Budget<'c>,
    pub mapped: Vec<RomSpan>,
}

impl<'a, 'b, 'c> Reader<'a, 'b, 'c> {
    pub fn new(bytes: &'a [u8], bank: usize, profile: Profile, budget: &'b mut Budget<'c>) -> Self {
        Self {
            bytes,
            bank,
            profile,
            legacy: false,
            cpu_base: 0x30000,
            budget,
            mapped: vec![profile.span()],
        }
    }

    pub fn legacy(
        bytes: &'a [u8],
        bank: usize,
        profile: Profile,
        budget: &'b mut Budget<'c>,
    ) -> Self {
        Self {
            legacy: true,
            cpu_base: u32::from(profile.segment) * 16,
            ..Self::new(bytes, bank, profile, budget)
        }
    }

    pub fn word(&mut self, at: usize) -> Result<u16, ReadError> {
        self.budget.charge()?;
        if at + 2 > 0x10000 {
            return Err(ReadError::Invalid);
        }
        let mut span = super::span(self.bank, at, 2);
        span.canonical_cpu_address = self.cpu_base + at as u32;
        self.mapped.push(span);
        super::word(self.bytes, self.bank * 0x10000 + at)
    }

    fn fixed(&self, address: usize, len: usize) -> Result<(), ReadError> {
        let start = self.profile.fixed & 0xffff;
        let end = if self.legacy {
            self.profile.end & 0xffff
        } else {
            usize::from(self.profile.init)
        };
        if address < start || address + len > end {
            return Err(ReadError::Invalid);
        }
        Ok(())
    }

    fn wave(&self, wave: u8) -> Result<(), ReadError> {
        let wave = if self.legacy { wave & 15 } else { wave };
        let at = usize::from(self.profile.wave) + usize::from(wave) * 16;
        self.fixed(at, 16)
    }

    fn envelope(&self, value: u8) -> Result<(), ReadError> {
        if self.legacy {
            return self.fixed(usize::from(self.profile.envelope), 240);
        }
        let at = usize::from(self.profile.envelope) + usize::from(value.wrapping_mul(2));
        self.fixed(at, 2)?;
        let pointer = usize::from(super::word(self.bytes, self.profile.offset(at as u16))?);
        self.fixed(pointer, 240)
    }

    pub fn track(&mut self, pointer: u16, channel: u8) -> Result<u32, ReadError> {
        let pointer = usize::from(pointer);
        let header = self.word(pointer)?;
        let settings = self.word(pointer + 2)?;
        let mode = (settings >> 8) as u8;
        if channel == 1 && mode != 0 {
            return Err(ReadError::Invalid);
        }
        self.wave((header >> 8) as u8)?;
        self.envelope(0)?;
        let mut state = State {
            index: 2,
            volume: settings as u8,
            ..State::default()
        };
        let mut visited = HashSet::new();
        let mut immediate = HashSet::new();
        let mut notes = 0_u32;
        for _ in 0..65_536 {
            self.budget.charge()?;
            if !immediate.insert(state) {
                return Err(ReadError::Invalid);
            }
            if !visited.insert(state) {
                return Ok(notes);
            }
            let at = pointer + usize::from(state.index) * 2;
            let command = self.word(at)?;
            let opcode = command as u8;
            let argument = (command >> 8) as u8;
            state.index = state.index.checked_add(1).ok_or(ReadError::Invalid)?;
            match opcode {
                0x00..=0x9f => {
                    if opcode & 15 < 12 {
                        if self.legacy {
                            self.fixed(usize::from(self.profile.frequency), 48)?;
                        } else {
                            let note = usize::from(opcode.wrapping_mul(2));
                            self.fixed(usize::from(self.profile.frequency) + note, 12)?;
                        }
                        if state.volume != 0 || state.volume_rise {
                            notes += 1;
                        }
                    }
                    immediate.clear();
                }
                0xa0 => state.volume = argument,
                0xa3 | 0xa5 | 0xaa | 0xab | 0xae | 0xaf | 0xc0..=0xcf | 0xe0..=0xfc | 0xfe => (),
                0xd0..=0xdf => state.volume_rise = opcode & 15 != 0,
                0xa1 => {
                    if argument != 0 && argument & 15 == 0 {
                        return Err(ReadError::Invalid);
                    }
                }
                0xa2 => self.wave(argument)?,
                0xa4 => {
                    if channel == 1 && argument != 0 {
                        return Err(ReadError::Invalid);
                    }
                }
                0xa6 if !self.legacy => {
                    if argument > 3 {
                        return Err(ReadError::Invalid);
                    }
                }
                0xa7 => immediate.clear(),
                0xa8 if !self.legacy => self.envelope(argument)?,
                0xa9 if !self.legacy => match argument {
                    0xf0 => state.variable = state.variable.wrapping_add(1),
                    0xf1 => state.variable = state.variable.wrapping_sub(1),
                    0xfe => return Err(ReadError::Invalid),
                    0xff => {
                        state.index = self.word(at + 2 + usize::from(state.variable) * 2)?;
                    }
                    value => state.variable = value,
                },
                0xac if !self.legacy => {
                    let target = self.word(at + 2)?;
                    if !target.is_multiple_of(2) {
                        return Err(ReadError::Invalid);
                    }
                    state.call_count = state.call_count.wrapping_sub(1);
                    if state.call_count == 0 {
                        state.index += 1;
                    } else {
                        if state.call_count == 255 {
                            state.call_count = argument;
                        }
                        state.return_index = state.index - 1;
                        state.index = target / 2;
                    }
                }
                0xad if !self.legacy => state.index = state.return_index,
                0xa6 | 0xa8 | 0xa9 | 0xac | 0xad => (),
                0xb0..=0xbf => {
                    let slot = usize::from(argument & 15);
                    if slot >= 4 {
                        return Err(ReadError::Invalid);
                    }
                    if opcode == 0xb0 {
                        if argument & 0xf0 == 0xf0 {
                            state.index = state.repeats[slot].1;
                        } else {
                            // The native handler uses the whole parameter in its slot offset.
                            if argument > 3 {
                                return Err(ReadError::Invalid);
                            }
                            state.repeats[slot].0 = argument & 15;
                        }
                    } else {
                        let repeat = &mut state.repeats[slot];
                        if repeat.0 == 0 {
                            repeat.0 = (opcode & 15) + 1;
                        }
                        repeat.0 -= 1;
                        if repeat.0 != 0 {
                            state.index = repeat.1;
                        }
                    }
                }
                0xfd => {
                    let slot = usize::from(argument & 15);
                    if slot >= 4 {
                        return Err(ReadError::Invalid);
                    }
                    state.repeats[slot].1 = state.index;
                }
                0xff => return Ok(notes),
            }
            if immediate.len() > 1024 {
                return Err(ReadError::Invalid);
            }
        }
        Err(ReadError::Invalid)
    }
}

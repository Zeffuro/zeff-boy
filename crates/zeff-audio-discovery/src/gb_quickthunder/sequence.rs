use std::collections::BTreeSet;

use crate::Budget;

use super::{ReadError, profiles::Driver, word};

pub(super) struct Reader<'a, 'b, 'c> {
    bytes: &'a [u8],
    driver: Driver,
    budget: &'b mut Budget<'c>,
    pub mapped: Vec<(usize, usize)>,
    effects: BTreeSet<(u32, u32, bool)>,
    instruments: BTreeSet<(u8, u8)>,
}

impl<'a, 'b, 'c> Reader<'a, 'b, 'c> {
    pub fn new(bytes: &'a [u8], driver: Driver, budget: &'b mut Budget<'c>) -> Self {
        let base = usize::from(driver.bank) * 0x4000;
        Self {
            bytes,
            driver,
            budget,
            mapped: vec![(base, base + driver.profile.len)],
            effects: BTreeSet::new(),
            instruments: BTreeSet::new(),
        }
    }

    pub fn read(&mut self, pointer: u32, len: usize) -> Result<&'a [u8], ReadError> {
        self.budget.charge()?;
        if pointer < 0x4000 || pointer + len as u32 > 0x8000 {
            return Err(ReadError::Invalid);
        }
        let at = self.driver.offset(pointer);
        self.mapped.push((at, at + len));
        self.bytes.get(at..at + len).ok_or(ReadError::Invalid)
    }

    fn byte(&mut self, pointer: u32) -> Result<u8, ReadError> {
        Ok(self.read(pointer, 1)?[0])
    }

    fn pointer(&mut self, pointer: u32) -> Result<u32, ReadError> {
        Ok(u32::from(word(self.read(pointer, 2)?, 0)))
    }

    pub fn effect(&mut self, mut pointer: u32, width: u32, wave: bool) -> Result<(), ReadError> {
        if !self.effects.insert((pointer, width, wave)) {
            return Ok(());
        }
        let mut visited = BTreeSet::new();
        for _ in 0..16384 {
            pointer += width;
            if !visited.insert(pointer) {
                return Ok(());
            }
            if self.byte(pointer)? == 0 {
                pointer = self.pointer(pointer + 1)?;
                self.byte(pointer)?;
            }
            let values = self.read(pointer + 1, width as usize)?;
            if wave && !(0x30..=0x3f).contains(&values[0]) {
                return Err(ReadError::Invalid);
            }
            pointer += 1;
        }
        Err(ReadError::Invalid)
    }

    fn instrument(&mut self, index: u8, channel: u8) -> Result<(), ReadError> {
        if !self.instruments.insert((index, channel)) {
            return Ok(());
        }
        let table = if channel == 3 {
            self.driver.noise
        } else {
            self.driver.instruments
        };
        let pointer = self.pointer(u32::from(table) + u32::from(index) * 2)?;
        let size = if channel == 2 {
            10
        } else if channel < 2 {
            9
        } else {
            7
        };
        let header = self.read(pointer, size)?.to_vec();
        let effects: &[(usize, u32)] = match channel {
            2 => &[(0, 1), (2, 1), (4, 1), (6, 2), (8, 1)],
            3 => &[(3, 1), (5, 1)],
            _ => &[(3, 1), (5, 2), (7, 1)],
        };
        for &(at, width) in effects {
            self.effect(u32::from(word(&header, at)), width, false)?;
        }
        Ok(())
    }

    pub fn track(&mut self, mut pattern: u32, channel: u8) -> Result<u32, ReadError> {
        let mut sequence = u32::from(self.driver.empty);
        let mut transpose = 0;
        let mut visited = BTreeSet::new();
        let mut notes = 0;
        let mut first = true;
        for _ in 0..32768 {
            self.budget.charge()?;
            if !visited.insert((pattern, sequence, transpose)) {
                return Ok(notes);
            }
            match self.byte(sequence)? {
                0 | 255 => {
                    if self.byte(sequence)? == 255 {
                        pattern = self.pointer(sequence + 1)?;
                    }
                    self.read(pattern, self.driver.profile.pattern_bytes as usize)?;
                    transpose = self.byte(pattern)?;
                    let index = if self.driver.profile.pattern_bytes == 4 {
                        self.pointer(pattern + 2)?
                    } else {
                        u32::from(self.byte(pattern + 1)?)
                    };
                    sequence = self.pointer(u32::from(self.driver.sequences) + index * 2)?;
                    pattern += self.driver.profile.pattern_bytes;
                    self.byte(sequence)?;
                }
                _ => (),
            }
            let note = self.byte(sequence + 1)?;
            let pitched = note < 0xfd || channel == 2 && note == 0xfd;
            if first && !pitched {
                return Err(ReadError::Invalid);
            }
            first = false;
            if pitched {
                let instrument = self.byte(sequence + 2)?;
                self.instrument(instrument, channel)?;
                notes += 1;
            }
            sequence += if pitched { 3 } else { 2 };
        }
        Err(ReadError::Invalid)
    }
}

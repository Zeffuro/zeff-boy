use super::{frontend::MemoryDrives, ram::Profile};
use crate::hardware::vdp::control_core::PhaseLevels;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Entry {
    pub position: u16,
    pub size: u8,
    pub link: u8,
}

impl Entry {
    fn validate(self) -> Result<()> {
        ensure!(
            self.position <= 0x3ff && self.size < 16 && self.link < 128,
            "invalid physical sprite attribute entry"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Change {
    pub index: u8,
    pub before: Entry,
    pub after: Entry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SpriteAttributes {
    entries: [Entry; 80],
    address: [u16; 2],
    high_bank: [bool; 2],
    upper: [bool; 2],
    lower: [bool; 2],
    high_data: [u8; 2],
}

impl SpriteAttributes {
    pub fn software_origin() -> Self {
        Self {
            entries: [Entry::default(); 80],
            address: [0; 2],
            high_bank: [false; 2],
            upper: [false; 2],
            lower: [false; 2],
            high_data: [0; 2],
        }
    }

    pub fn clock(
        &mut self,
        phases: PhaseLevels,
        profile: Profile,
        memory: MemoryDrives,
    ) -> Result<Option<Change>> {
        profile.validate()?;
        ensure!(
            memory.address <= 0x1ffff,
            "invalid sprite attribute bus address"
        );
        let mut change = None;
        if phases.horizontal_master {
            // The delayed write consumes old slaves before capturing the next bus image.
            if self.upper[1] || self.lower[1] {
                let index =
                    ((self.address[1] >> 3) & 63) as u8 | if self.high_bank[1] { 64 } else { 0 };
                if index < 80 {
                    let entry = &mut self.entries[usize::from(index)];
                    let before = *entry;
                    if self.address[1] & 2 != 0 {
                        if self.upper[1] {
                            entry.size = self.high_data[1];
                        }
                        if self.lower[1] {
                            entry.link = memory.data_low_delay & 127;
                        }
                    } else {
                        if self.upper[1] {
                            entry.position =
                                entry.position & 255 | u16::from(self.high_data[1] & 3) << 8;
                        }
                        if self.lower[1] {
                            entry.position =
                                entry.position & 0x300 | u16::from(memory.data_low_delay);
                        }
                    }
                    change = Some(Change {
                        index,
                        before,
                        after: *entry,
                    });
                }
            }
            let wide = profile.registers[12] & 1 != 0;
            let mask_bit = u8::from(wide);
            let selected = (profile.registers[5] | mask_bit)
                == (((memory.address >> 9) & 255) as u8 | mask_bit)
                && memory.address & 4 == 0
                && profile.registers[1] & 4 != 0;
            self.address[0] = (memory.address & 0x1fa) as u16;
            self.high_bank[0] = wide && memory.address & 512 != 0;
            self.upper[0] = selected && memory.upper_byte;
            self.lower[0] = selected && memory.lower_byte;
            self.high_data[0] = ((memory.data >> 8) & 15) as u8;
        }
        if phases.horizontal_slave {
            self.address[1] = self.address[0];
            self.high_bank[1] = self.high_bank[0];
            self.upper[1] = self.upper[0];
            self.lower[1] = self.lower[0];
            self.high_data[1] = self.high_data[0];
        }
        Ok(change)
    }

    pub fn entry(&self, index: u8) -> Option<Entry> {
        self.entries.get(usize::from(index)).copied()
    }

    pub fn selected_index(&self, scan: u8, read: bool) -> u8 {
        if read || self.upper[1] || self.lower[1] {
            ((self.address[1] >> 3) & 63) as u8 | if self.high_bank[1] { 64 } else { 0 }
        } else {
            scan
        }
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(1);
        for entry in self.entries {
            writer.write_u16(entry.position);
            writer.write_u8(entry.size);
            writer.write_u8(entry.link);
        }
        for address in self.address {
            writer.write_u16(address);
        }
        for value in self
            .high_bank
            .into_iter()
            .chain(self.upper)
            .chain(self.lower)
        {
            writer.write_bool(value);
        }
        writer.write_bytes(&self.high_data);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        ensure!(
            reader.read_u8()? == 1,
            "unsupported sprite attribute state version"
        );
        let mut entries = [Entry::default(); 80];
        for entry in &mut entries {
            *entry = Entry {
                position: reader.read_u16()?,
                size: reader.read_u8()?,
                link: reader.read_u8()?,
            };
            entry.validate()?;
        }
        let address = [reader.read_u16()?, reader.read_u16()?];
        let high_bank = [reader.read_bool()?, reader.read_bool()?];
        let upper = [reader.read_bool()?, reader.read_bool()?];
        let lower = [reader.read_bool()?, reader.read_bool()?];
        let mut high_data = [0; 2];
        reader.read_exact(&mut high_data)?;
        ensure!(
            address.into_iter().all(|value| value & !0x1fa == 0)
                && high_data.into_iter().all(|value| value < 16),
            "invalid sprite attribute capture state"
        );
        Ok(Self {
            entries,
            address,
            high_bank,
            upper,
            lower,
            high_data,
        })
    }
}

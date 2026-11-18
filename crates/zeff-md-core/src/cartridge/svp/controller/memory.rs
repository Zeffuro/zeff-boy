use super::{Channel, Controller, Fault};
use std::sync::Arc;

impl Controller {
    pub(in super::super) fn main_dram_write(&mut self, address: u16, value: u16, mask: u16) {
        let word = self.dram.read(address);
        self.dram.write(address, (word & !mask) | (value & mask));
        self.cold_blind = false;
    }

    pub(super) fn channel(&self, index: u8, writing: bool) -> Result<Channel, Fault> {
        self.channels[usize::from(writing)][usize::from(index)]
            .ok_or_else(|| Self::failure(index, "unprogrammed memory channel"))
    }

    pub(super) fn read_memory(&mut self, index: u8, captured: Option<u16>) -> Result<u16, Fault> {
        let channel = self.channel(index, false)?;
        let address = channel.address as usize;
        let value = match captured {
            Some(value) => Some(value),
            None => match address {
                0..=0x0fffff => self.rom.get(address).copied(),
                0x180000..=0x18ffff => Some(self.dram.read((address - 0x180000) as u16)),
                0x1c8000..=0x1c83ff => self.iram.get(address - 0x1c8000).copied(),
                _ => None,
            },
        }
        .ok_or_else(|| Self::failure(index, "unmapped external memory read"))?;
        self.channels[0][usize::from(index)] = Some(channel.next());
        self.pmc_shadow = Some(channel.next());
        self.cold_blind = false;
        Ok(value)
    }

    pub(super) fn write_memory(
        &mut self,
        index: u8,
        value: u16,
        captured: Option<u16>,
    ) -> Result<u16, Fault> {
        let channel = self.channel(index, true)?;
        let address = channel.address as usize;
        if channel.mode & 0x4400 != 0 && !(0x180000..=0x18ffff).contains(&address) {
            return Err(Self::failure(index, "unmapped DRAM write mode"));
        }
        let written = match address {
            0x180000..=0x18ffff => {
                let address = (address - 0x180000) as u16;
                let value = if let Some(captured) = captured {
                    captured
                } else if channel.mode & 0x0400 != 0 {
                    let mut merged = self.dram.read(address);
                    for shift in [0, 4, 8, 12] {
                        let mask = 0xf << shift;
                        if value & mask != 0 {
                            merged = (merged & !mask) | (value & mask);
                        }
                    }
                    merged
                } else {
                    value
                };
                self.dram.write(address, value);
                value
            }
            0x1c8000..=0x1c83ff => {
                Arc::make_mut(&mut self.iram)[address - 0x1c8000] = value;
                value
            }
            _ => return Err(Self::failure(index, "unmapped external memory write")),
        };
        self.channels[1][usize::from(index)] = Some(channel.next());
        self.pmc_shadow = Some(channel.next());
        self.cold_blind = false;
        Ok(written)
    }
}

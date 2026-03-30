use super::CartridgeDebugInfo;
use super::{
    MAX_SAVE_RAM, build_debug_info, is_ram_enable, load_ram_into, read_banked_ram, write_banked_ram,
};
use crate::save_state::{StateReader, StateWriter, StateWriterGbExt};
use anyhow::Result;

pub struct HuC1 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    ram_enable: bool,
    rom_bank: usize,
    ram_bank: usize,
    banking_mode: bool,
    rom_high_address: usize,
}

impl HuC1 {
    pub fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        Self {
            rom,
            ram: vec![0; ram_size],
            ram_enable: false,
            rom_bank: 1,
            ram_bank: 0,
            banking_mode: false,
            rom_high_address: 0,
        }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom.get(addr as usize).copied().unwrap_or(0xFF),
            0x4000..=0x7FFF => {
                let mut physical = self.rom_bank << 14;
                if !self.banking_mode {
                    physical |= self.rom_high_address << 19;
                }
                physical |= (addr & 0x3FFF) as usize;
                physical %= self.rom.len().max(1);
                self.rom.get(physical).copied().unwrap_or(0xFF)
            }
            _ => 0xFF,
        }
    }

    pub fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enable = is_ram_enable(value),
            0x2000..=0x3FFF => {
                let bank = (value & 0x3F) as usize;
                self.rom_bank = if bank == 0 { 1 } else { bank };
            }
            0x4000..=0x5FFF => {
                if self.banking_mode {
                    self.ram_bank = (value & 0x03) as usize;
                } else {
                    self.rom_high_address = (value & 0x03) as usize;
                }
            }
            0x6000..=0x7FFF => self.banking_mode = (value & 0x01) != 0,
            _ => {}
        }
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enable || self.ram.is_empty() {
            return 0xFF;
        }
        let bank = if self.banking_mode { self.ram_bank } else { 0 };
        read_banked_ram(&self.ram, bank, addr)
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enable || self.ram.is_empty() {
            return;
        }
        let bank = if self.banking_mode { self.ram_bank } else { 0 };
        write_banked_ram(&mut self.ram, bank, addr, value);
    }

    pub fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info(
            "HuC1",
            self.rom_bank,
            if self.banking_mode { self.ram_bank } else { 0 },
            self.ram_enable,
            Some(self.banking_mode),
        )
    }

    pub fn restore_rom_bytes(&mut self, rom: Vec<u8>) {
        self.rom = rom;
    }

    pub(super) fn ram_bytes(&self) -> &[u8] {
        &self.ram
    }

    pub(super) fn load_ram_bytes(&mut self, bytes: &[u8]) {
        load_ram_into(&mut self.ram, bytes);
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_len(self.ram.len());
        writer.write_bytes(&self.ram);
        writer.write_bool(self.ram_enable);
        writer.write_u64(self.rom_bank as u64);
        writer.write_u64(self.ram_bank as u64);
        writer.write_bool(self.banking_mode);
        writer.write_u64(self.rom_high_address as u64);
    }

    pub(super) fn bess_mbc_writes(&self) -> Vec<(u16, u8)> {
        vec![
            (0x0000, if self.ram_enable { 0x0A } else { 0x00 }),
            (0x2000, self.rom_bank as u8),
            (0x4000, self.ram_bank as u8),
            (0x6000, u8::from(self.banking_mode)),
        ]
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            rom: Vec::new(),
            ram: reader.read_vec(MAX_SAVE_RAM)?,
            ram_enable: reader.read_bool()?,
            rom_bank: reader.read_u64()? as usize,
            ram_bank: reader.read_u64()? as usize,
            banking_mode: reader.read_bool()?,
            rom_high_address: reader.read_u64()? as usize,
        })
    }
}

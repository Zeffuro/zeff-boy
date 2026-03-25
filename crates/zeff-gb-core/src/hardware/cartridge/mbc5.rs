use super::CartridgeDebugInfo;
use super::{
    MAX_SAVE_RAM, build_debug_info, is_ram_enable, load_ram_into, read_banked_ram, read_banked_rom,
    read_fixed_rom, write_banked_ram,
};
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

pub struct Mbc5 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    ram_enable: bool,
    rom_bank: usize,
    ram_bank: usize,
    has_rumble: bool,
    rumble_active: bool,
}

impl Mbc5 {
    pub fn new(rom: Vec<u8>, ram_size: usize, has_rumble: bool) -> Self {
        Self {
            rom,
            ram: vec![0; ram_size],
            ram_enable: false,
            rom_bank: 1,
            ram_bank: 0,
            has_rumble,
            rumble_active: false,
        }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => read_fixed_rom(&self.rom, addr),
            0x4000..=0x7FFF => read_banked_rom(&self.rom, self.rom_bank, addr),
            _ => 0xFF,
        }
    }

    pub fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enable = is_ram_enable(value),
            0x2000..=0x2FFF => self.rom_bank = (self.rom_bank & 0x100) | value as usize,
            0x3000..=0x3FFF => {
                self.rom_bank = (self.rom_bank & 0x0FF) | (((value & 0x01) as usize) << 8)
            }
            0x4000..=0x5FFF => {
                if self.has_rumble {
                    self.rumble_active = value & 0x08 != 0;
                    self.ram_bank = (value & 0x07) as usize;
                } else {
                    self.ram_bank = (value & 0x0F) as usize;
                }
            }
            _ => {}
        }
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enable || self.ram.is_empty() {
            return 0xFF;
        }
        read_banked_ram(&self.ram, self.ram_bank, addr)
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enable || self.ram.is_empty() {
            return;
        }
        write_banked_ram(&mut self.ram, self.ram_bank, addr, value);
    }

    pub fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub fn rumble_active(&self) -> bool {
        self.rumble_active
    }

    pub fn set_has_rumble(&mut self, has_rumble: bool) {
        self.has_rumble = has_rumble;
    }

    pub fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info(
            if self.has_rumble {
                "MBC5+Rumble"
            } else {
                "MBC5"
            },
            self.rom_bank,
            self.ram_bank,
            self.ram_enable,
            None,
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
    }

    pub(super) fn bess_mbc_writes(&self) -> Vec<(u16, u8)> {
        vec![
            (0x0000, if self.ram_enable { 0x0A } else { 0x00 }),
            (0x2000, (self.rom_bank & 0xFF) as u8),
            (0x3000, ((self.rom_bank >> 8) & 0x01) as u8),
            (0x4000, self.ram_bank as u8),
        ]
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            rom: Vec::new(),
            ram: reader.read_vec(MAX_SAVE_RAM)?,
            ram_enable: reader.read_bool()?,
            rom_bank: reader.read_u64()? as usize,
            ram_bank: reader.read_u64()? as usize,
            has_rumble: false,
            rumble_active: false,
        })
    }
}

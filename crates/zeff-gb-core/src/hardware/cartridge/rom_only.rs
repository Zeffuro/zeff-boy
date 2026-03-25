use super::CartridgeDebugInfo;
use super::{MAX_SAVE_RAM, build_debug_info, load_ram_into, read_banked_ram, write_banked_ram};
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

pub struct RomOnly {
    rom: Vec<u8>,
    ram: Vec<u8>,
}

impl RomOnly {
    pub fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        Self {
            rom,
            ram: vec![0; ram_size],
        }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        self.rom.get(addr as usize).copied().unwrap_or(0xFF)
    }

    pub fn write_rom(&mut self, _addr: u16, _value: u8) {}

    pub fn read_ram(&self, addr: u16) -> u8 {
        read_banked_ram(&self.ram, 0, addr)
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        write_banked_ram(&mut self.ram, 0, addr, value);
    }

    pub fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info("ROM_ONLY", 0, 0, !self.ram.is_empty(), None)
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
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            rom: Vec::new(),
            ram: reader.read_vec(MAX_SAVE_RAM)?,
        })
    }
}

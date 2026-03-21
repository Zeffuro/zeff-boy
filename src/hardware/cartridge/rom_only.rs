use super::CartridgeDebugInfo;
use super::{build_debug_info, read_banked_ram, write_banked_ram};

pub(crate) struct RomOnly {
    rom: Vec<u8>,
    ram: Vec<u8>,
}

impl RomOnly {
    pub(crate) fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        Self {
            rom,
            ram: vec![0; ram_size],
        }
    }

    pub(crate) fn read_rom(&self, addr: u16) -> u8 {
        self.rom.get(addr as usize).copied().unwrap_or(0xFF)
    }

    pub(crate) fn write_rom(&mut self, _addr: u16, _value: u8) {}

    pub(crate) fn read_ram(&self, addr: u16) -> u8 {
        read_banked_ram(&self.ram, 0, addr)
    }

    pub(crate) fn write_ram(&mut self, addr: u16, value: u8) {
        write_banked_ram(&mut self.ram, 0, addr, value);
    }

    pub(crate) fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub(crate) fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info("ROM_ONLY", 0, 0, !self.ram.is_empty(), None)
    }
}

use super::{build_debug_info, read_banked_rom, read_fixed_rom};
use super::CartridgeDebugInfo;

pub(crate) struct Mbc2 {
    rom: Vec<u8>,
    ram: [u8; 0x200],
    ram_enable: bool,
    rom_bank: usize,
}

impl Mbc2 {
    pub(crate) fn new(rom: Vec<u8>) -> Self {
        Self {
            rom,
            ram: [0; 0x200],
            ram_enable: false,
            rom_bank: 1,
        }
    }

    pub(crate) fn read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => read_fixed_rom(&self.rom, addr),
            0x4000..=0x7FFF => read_banked_rom(&self.rom, self.rom_bank, addr),
            _ => 0xFF,
        }
    }

    pub(crate) fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => {
                if addr & 0x0100 == 0 {
                    self.ram_enable = (value & 0x0F) == 0x0A;
                }
            }
            0x2000..=0x3FFF => {
                if addr & 0x0100 != 0 {
                    let bank = (value & 0x0F) as usize;
                    self.rom_bank = if bank == 0 { 1 } else { bank };
                }
            }
            _ => {}
        }
    }

    pub(crate) fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enable {
            return 0xFF;
        }
        let idx = ((addr - 0xA000) as usize) & 0x01FF;
        0xF0 | (self.ram[idx] & 0x0F)
    }

    pub(crate) fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enable {
            return;
        }
        let idx = ((addr - 0xA000) as usize) & 0x01FF;
        self.ram[idx] = value & 0x0F;
    }

    pub(crate) fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub(crate) fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info("MBC2", self.rom_bank, 0, self.ram_enable, None)
    }
}


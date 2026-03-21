use super::CartridgeDebugInfo;
use super::{build_debug_info, read_banked_ram, read_banked_rom, read_fixed_rom, write_banked_ram};

pub(crate) struct Mbc3 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    ram_enable: bool,
    rom_bank: usize,
    ram_or_rtc_select: u8,
    rtc_latch_write: u8,
}

impl Mbc3 {
    pub(crate) fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        Self {
            rom,
            ram: vec![0; ram_size],
            ram_enable: false,
            rom_bank: 1,
            ram_or_rtc_select: 0,
            rtc_latch_write: 0,
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
            0x0000..=0x1FFF => self.ram_enable = (value & 0x0F) == 0x0A,
            0x2000..=0x3FFF => {
                let bank = (value & 0x7F) as usize;
                self.rom_bank = if bank == 0 { 1 } else { bank };
            }
            0x4000..=0x5FFF => self.ram_or_rtc_select = value,
            0x6000..=0x7FFF => self.rtc_latch_write = value,
            _ => {}
        }
    }

    pub(crate) fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enable {
            return 0xFF;
        }
        if (0x08..=0x0C).contains(&self.ram_or_rtc_select) {
            return 0;
        }
        if self.ram.is_empty() {
            return 0xFF;
        }

        let bank = (self.ram_or_rtc_select & 0x03) as usize;
        read_banked_ram(&self.ram, bank, addr)
    }

    pub(crate) fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enable || self.ram.is_empty() {
            return;
        }
        if (0x08..=0x0C).contains(&self.ram_or_rtc_select) {
            return;
        }

        let bank = (self.ram_or_rtc_select & 0x03) as usize;
        write_banked_ram(&mut self.ram, bank, addr, value);
    }

    pub(crate) fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub(crate) fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info(
            "MBC3",
            self.rom_bank,
            (self.ram_or_rtc_select & 0x03) as usize,
            self.ram_enable,
            None,
        )
    }
}

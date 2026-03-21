use super::CartridgeDebugInfo;
use super::{build_debug_info, read_banked_ram, write_banked_ram};

pub(crate) struct Mbc1 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    ram_enable: bool,
    rom_bank: usize,
    ram_bank: usize,
    banking_mode: bool,
    rom_bank_mask: usize,
}

impl Mbc1 {
    pub(crate) fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        let num_banks = (rom.len() / 0x4000).max(1);
        let rom_bank_mask = num_banks.next_power_of_two() - 1;

        Self {
            rom,
            ram: vec![0; ram_size],
            ram_enable: false,
            rom_bank: 1,
            ram_bank: 0,
            banking_mode: false,
            rom_bank_mask,
        }
    }

    pub(crate) fn read_rom(&self, addr: u16) -> u8 {
        let bank = match addr {
            0x0000..=0x3FFF => {
                if self.banking_mode {
                    self.ram_bank << 5
                } else {
                    0
                }
            }
            0x4000..=0x7FFF => {
                let b = if self.rom_bank == 0 { 1 } else { self.rom_bank };
                b | (self.ram_bank << 5)
            }
            _ => 0,
        };

        let physical_bank = bank & self.rom_bank_mask;
        let physical_addr = (physical_bank * 0x4000) | ((addr & 0x3FFF) as usize);
        self.rom.get(physical_addr).copied().unwrap_or(0xFF)
    }

    pub(crate) fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enable = (value & 0x0F) == 0x0A,
            0x2000..=0x3FFF => self.rom_bank = (value & 0x1F) as usize,
            0x4000..=0x5FFF => self.ram_bank = (value & 0x03) as usize,
            0x6000..=0x7FFF => self.banking_mode = (value & 0x01) != 0,
            _ => {}
        }
    }

    pub(crate) fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enable || self.ram.is_empty() {
            return 0xFF;
        }

        let bank = if self.banking_mode { self.ram_bank } else { 0 };
        read_banked_ram(&self.ram, bank, addr)
    }

    pub(crate) fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enable || self.ram.is_empty() {
            return;
        }

        let bank = if self.banking_mode { self.ram_bank } else { 0 };
        write_banked_ram(&mut self.ram, bank, addr, value);
    }

    pub(crate) fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub(crate) fn debug_info(&self) -> CartridgeDebugInfo {
        let low_bank = if self.rom_bank == 0 { 1 } else { self.rom_bank };
        build_debug_info(
            "MBC1",
            (low_bank | (self.ram_bank << 5)) & self.rom_bank_mask,
            if self.banking_mode { self.ram_bank } else { 0 },
            self.ram_enable,
            Some(self.banking_mode),
        )
    }
}

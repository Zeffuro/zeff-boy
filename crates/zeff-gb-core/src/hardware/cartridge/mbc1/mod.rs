use super::CartridgeDebugInfo;
use super::{
    MAX_SAVE_RAM, RAM_BANK_SIZE, ROM_BANK_SIZE, build_debug_info, is_ram_enable, load_ram_into,
    read_banked_ram, write_banked_ram,
};
use crate::save_state::{StateReader, StateWriter, StateWriterGbExt};
use anyhow::Result;

pub struct Mbc1 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    ram_enable: bool,
    rom_bank: usize,
    ram_bank: usize,
    banking_mode: bool,
    rom_bank_mask: usize,
    multicart: bool,
}

impl Mbc1 {
    pub fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        let num_banks = (rom.len() / ROM_BANK_SIZE).max(1);
        let rom_bank_mask = num_banks.next_power_of_two() - 1;
        let multicart = detect_mbc1_multicart(&rom);

        Self {
            rom,
            ram: vec![0; ram_size],
            ram_enable: false,
            rom_bank: 1,
            ram_bank: 0,
            banking_mode: false,
            rom_bank_mask,
            multicart,
        }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        let bank = match addr {
            0x0000..=0x3FFF if self.banking_mode => self.fixed_area_rom_bank(),
            0x4000..=0x7FFF => self.switchable_rom_bank(),
            _ => 0,
        };

        let physical_bank = bank & self.rom_bank_mask;
        let physical_addr = (physical_bank * ROM_BANK_SIZE) | ((addr & 0x3FFF) as usize);
        self.rom.get(physical_addr).copied().unwrap_or(0xFF)
    }

    pub fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enable = is_ram_enable(value),
            0x2000..=0x3FFF => self.rom_bank = (value & 0x1F) as usize,
            0x4000..=0x5FFF => self.ram_bank = (value & 0x03) as usize,
            0x6000..=0x7FFF => self.banking_mode = (value & 0x01) != 0,
            _ => {}
        }
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enable || self.ram.is_empty() {
            return 0xFF;
        }

        let bank = self.effective_ram_bank();
        read_banked_ram(&self.ram, bank, addr)
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enable || self.ram.is_empty() {
            return;
        }

        let bank = self.effective_ram_bank();
        write_banked_ram(&mut self.ram, bank, addr, value);
    }

    pub fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info(
            "MBC1",
            self.switchable_rom_bank() & self.rom_bank_mask,
            self.effective_ram_bank(),
            self.ram_enable,
            Some(self.banking_mode),
        )
    }

    pub fn restore_rom_bytes(&mut self, rom: Vec<u8>) {
        self.rom = rom;
        let num_banks = (self.rom.len() / ROM_BANK_SIZE).max(1);
        self.rom_bank_mask = num_banks.next_power_of_two() - 1;
        self.multicart = detect_mbc1_multicart(&self.rom);
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
        writer.write_u64(self.rom_bank_mask as u64);
    }

    pub(super) fn bess_mbc_writes(&self) -> Vec<(u16, u8)> {
        vec![
            (0x0000, if self.ram_enable { 0x0A } else { 0x00 }),
            (0x2000, (self.rom_bank & 0x1F) as u8),
            (0x4000, (self.ram_bank & 0x03) as u8),
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
            rom_bank_mask: reader.read_u64()? as usize,
            multicart: false,
        })
    }

    fn fixed_area_rom_bank(&self) -> usize {
        if self.multicart {
            self.ram_bank << 4
        } else {
            self.ram_bank << 5
        }
    }

    fn switchable_rom_bank(&self) -> usize {
        let low_bank = if self.rom_bank == 0 { 1 } else { self.rom_bank };
        if self.multicart {
            (self.ram_bank << 4) | (low_bank & 0x0F)
        } else {
            low_bank | (self.ram_bank << 5)
        }
    }

    fn effective_ram_bank(&self) -> usize {
        if !self.banking_mode {
            return 0;
        }

        let bank_count = (self.ram.len() / RAM_BANK_SIZE).max(1);
        self.ram_bank % bank_count
    }
}

fn detect_mbc1_multicart(rom: &[u8]) -> bool {
    rom.len() == 64 * ROM_BANK_SIZE
        && [0, 16, 32, 48]
            .into_iter()
            .all(|bank| bank_has_nintendo_logo(rom, bank))
}

fn bank_has_nintendo_logo(rom: &[u8], bank: usize) -> bool {
    let start = bank * ROM_BANK_SIZE + 0x0104;
    let end = start + NINTENDO_LOGO.len();
    rom.get(start..end) == Some(NINTENDO_LOGO.as_slice())
}

const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

#[cfg(test)]
mod tests;

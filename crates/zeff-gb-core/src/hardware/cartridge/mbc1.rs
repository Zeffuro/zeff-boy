use super::CartridgeDebugInfo;
use super::{
    MAX_SAVE_RAM, ROM_BANK_SIZE, build_debug_info, is_ram_enable, load_ram_into, read_banked_ram,
    write_banked_ram,
};
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

pub struct Mbc1 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    ram_enable: bool,
    rom_bank: usize,
    ram_bank: usize,
    banking_mode: bool,
    rom_bank_mask: usize,
}

impl Mbc1 {
    pub fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        let num_banks = (rom.len() / ROM_BANK_SIZE).max(1);
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

    pub fn read_rom(&self, addr: u16) -> u8 {
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
        let low_bank = if self.rom_bank == 0 { 1 } else { self.rom_bank };
        build_debug_info(
            "MBC1",
            (low_bank | (self.ram_bank << 5)) & self.rom_bank_mask,
            if self.banking_mode { self.ram_bank } else { 0 },
            self.ram_enable,
            Some(self.banking_mode),
        )
    }

    pub fn restore_rom_bytes(&mut self, rom: Vec<u8>) {
        self.rom = rom;
        let num_banks = (self.rom.len() / ROM_BANK_SIZE).max(1);
        self.rom_bank_mask = num_banks.next_power_of_two() - 1;
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mbc1(n_banks: usize, ram_size: usize) -> Mbc1 {
        let mut rom = vec![0u8; n_banks * ROM_BANK_SIZE];
        for bank in 0..n_banks {
            let start = bank * ROM_BANK_SIZE;
            for byte in &mut rom[start..start + ROM_BANK_SIZE] {
                *byte = bank as u8;
            }
        }
        Mbc1::new(rom, ram_size)
    }

    #[test]
    fn default_bank_is_1() {
        let mbc = make_mbc1(4, 0);
        assert_eq!(mbc.read_rom(0x4000), 1);
    }

    #[test]
    fn bank_0_corrected_to_1() {
        let mut mbc = make_mbc1(8, 0);
        mbc.write_rom(0x2000, 0x00);
        assert_eq!(mbc.read_rom(0x4000), 1);
    }

    #[test]
    fn bank_switching_basic() {
        let mut mbc = make_mbc1(8, 0);
        mbc.write_rom(0x2000, 3);
        assert_eq!(mbc.read_rom(0x4000), 3);
        mbc.write_rom(0x2000, 7);
        assert_eq!(mbc.read_rom(0x4000), 7);
    }

    #[test]
    fn bank_number_masked_to_5_bits() {
        let mut mbc = make_mbc1(32, 0);
        mbc.write_rom(0x2000, 0xFF);
        assert_eq!(mbc.read_rom(0x4000), 31);
    }

    #[test]
    fn ram_disabled_by_default() {
        let mbc = make_mbc1(4, 0x2000);
        assert_eq!(mbc.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn ram_enable_disable() {
        let mut mbc = make_mbc1(4, 0x2000);
        mbc.write_rom(0x0000, 0x0A);
        mbc.write_ram(0xA000, 0x42);
        assert_eq!(mbc.read_ram(0xA000), 0x42);

        mbc.write_rom(0x0000, 0x00);
        assert_eq!(mbc.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn ram_write_ignored_when_disabled() {
        let mut mbc = make_mbc1(4, 0x2000);
        mbc.write_rom(0x0000, 0x0A);
        mbc.write_ram(0xA000, 0x42);
        mbc.write_rom(0x0000, 0x00);
        mbc.write_ram(0xA000, 0xFF);
        mbc.write_rom(0x0000, 0x0A);
        assert_eq!(mbc.read_ram(0xA000), 0x42);
    }

    #[test]
    fn banking_mode_0_uses_bank_0_for_low_rom() {
        let mbc = make_mbc1(64, 0);
        assert_eq!(mbc.read_rom(0x0000), 0);
    }

    #[test]
    fn banking_mode_1_uses_ram_bank_for_low_rom() {
        let mut mbc = make_mbc1(64, 0);
        mbc.write_rom(0x6000, 0x01);
        mbc.write_rom(0x4000, 0x01);
        assert_eq!(mbc.read_rom(0x0000), 32);
    }

    #[test]
    fn banking_mode_1_ram_bank_switching() {
        let mut mbc = make_mbc1(4, 0x8000);
        mbc.write_rom(0x0000, 0x0A);
        mbc.write_rom(0x6000, 0x01);

        mbc.write_rom(0x4000, 0x00);
        mbc.write_ram(0xA000, 0xAA);

        mbc.write_rom(0x4000, 0x01);
        mbc.write_ram(0xA000, 0xBB);

        mbc.write_rom(0x4000, 0x00);
        assert_eq!(mbc.read_ram(0xA000), 0xAA);

        mbc.write_rom(0x4000, 0x01);
        assert_eq!(mbc.read_ram(0xA000), 0xBB);
    }

    #[test]
    fn banking_mode_0_always_uses_ram_bank_0() {
        let mut mbc = make_mbc1(4, 0x8000);
        mbc.write_rom(0x0000, 0x0A);
        mbc.write_rom(0x4000, 0x01);
        mbc.write_ram(0xA000, 0x42);

        mbc.write_rom(0x4000, 0x00);
        assert_eq!(mbc.read_ram(0xA000), 0x42);
    }

    #[test]
    fn rom_bank_mask_prevents_out_of_bounds() {
        let mut mbc = make_mbc1(4, 0);
        mbc.write_rom(0x2000, 7);
        assert_eq!(mbc.read_rom(0x4000), 3);
    }

    #[test]
    fn no_ram_returns_ff() {
        let mut mbc = make_mbc1(4, 0);
        mbc.write_rom(0x0000, 0x0A);
        assert_eq!(mbc.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn save_state_roundtrip() {
        let mut mbc = make_mbc1(8, 0x8000);
        mbc.write_rom(0x0000, 0x0A);
        mbc.write_rom(0x2000, 5);
        mbc.write_rom(0x6000, 0x01);
        mbc.write_rom(0x4000, 0x01);
        mbc.write_ram(0xA000, 0x42);

        let mut writer = StateWriter::new();
        mbc.write_state(&mut writer);
        let data = writer.into_bytes();
        let mut reader = StateReader::new(&data);
        let mut restored = Mbc1::read_state(&mut reader).unwrap();
        restored.restore_rom_bytes(mbc.rom.clone());

        assert_eq!(restored.read_rom(0x4000), mbc.read_rom(0x4000));
        assert_eq!(restored.read_ram(0xA000), 0x42);
        assert_eq!(restored.banking_mode, true);
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mbc5(n_banks: usize, ram_size: usize, has_rumble: bool) -> Mbc5 {
        let mut rom = vec![0u8; n_banks * 0x4000];
        for bank in 0..n_banks {
            let start = bank * 0x4000;
            for byte in &mut rom[start..start + 0x4000] {
                *byte = (bank & 0xFF) as u8;
            }
        }
        Mbc5::new(rom, ram_size, has_rumble)
    }

    #[test]
    fn default_bank_is_1() {
        let mbc = make_mbc5(4, 0, false);
        assert_eq!(mbc.read_rom(0x4000), 1);
    }

    #[test]
    fn bank_0_is_valid_for_mbc5() {
        let mut mbc = make_mbc5(4, 0, false);
        mbc.write_rom(0x2000, 0x00);
        assert_eq!(mbc.read_rom(0x4000), 0);
    }

    #[test]
    fn bank_switching_low_byte() {
        let mut mbc = make_mbc5(256, 0, false);
        mbc.write_rom(0x2000, 0xFF);
        assert_eq!(mbc.read_rom(0x4000), 0xFF);
    }

    #[test]
    fn bank_switching_9_bit() {
        let mut mbc = make_mbc5(512, 0, false);
        mbc.write_rom(0x2000, 0x00);
        mbc.write_rom(0x3000, 0x01);
        assert_eq!(mbc.read_rom(0x4000), 0);
        assert_eq!(mbc.rom_bank, 256);
    }

    #[test]
    fn high_bank_bit_only_uses_bit_0() {
        let mut mbc = make_mbc5(512, 0, false);
        mbc.write_rom(0x3000, 0xFF);
        assert_eq!(mbc.rom_bank & 0x100, 0x100);
    }

    #[test]
    fn ram_enable_disable() {
        let mut mbc = make_mbc5(4, 0x2000, false);
        mbc.write_rom(0x0000, 0x0A);
        mbc.write_ram(0xA000, 0x42);
        assert_eq!(mbc.read_ram(0xA000), 0x42);

        mbc.write_rom(0x0000, 0x00);
        assert_eq!(mbc.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn ram_bank_switching() {
        let mut mbc = make_mbc5(4, 0x8000, false);
        mbc.write_rom(0x0000, 0x0A);

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
    fn ram_bank_uses_4_bits_no_rumble() {
        let mut mbc = make_mbc5(4, 0x20000, false);
        mbc.write_rom(0x4000, 0xFF);
        assert_eq!(mbc.ram_bank, 15);
    }

    #[test]
    fn rumble_bit_used_in_ram_bank_register() {
        let mut mbc = make_mbc5(4, 0x8000, true);
        mbc.write_rom(0x4000, 0x08);
        assert!(mbc.rumble_active());
        assert_eq!(mbc.ram_bank, 0);

        mbc.write_rom(0x4000, 0x0B);
        assert!(mbc.rumble_active());
        assert_eq!(mbc.ram_bank, 3);
    }

    #[test]
    fn no_ram_returns_ff() {
        let mut mbc = make_mbc5(4, 0, false);
        mbc.write_rom(0x0000, 0x0A);
        assert_eq!(mbc.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn save_state_roundtrip() {
        let mut mbc = make_mbc5(8, 0x2000, false);
        mbc.write_rom(0x0000, 0x0A);
        mbc.write_rom(0x2000, 5);
        mbc.write_ram(0xA000, 0x42);

        let mut writer = StateWriter::new();
        mbc.write_state(&mut writer);
        let data = writer.into_bytes();
        let mut reader = StateReader::new(&data);
        let mut restored = Mbc5::read_state(&mut reader).unwrap();
        restored.restore_rom_bytes(mbc.rom.clone());

        assert_eq!(restored.read_rom(0x4000), 5);
        assert_eq!(restored.read_ram(0xA000), 0x42);
    }
}

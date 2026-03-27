use super::CartridgeDebugInfo;
use super::{
    RAM_BASE_ADDR, build_debug_info, is_ram_enable, load_ram_into, read_banked_rom, read_fixed_rom,
};
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

pub struct Mbc2 {
    rom: Vec<u8>,
    ram: [u8; 0x200],
    ram_enable: bool,
    rom_bank: usize,
}

impl Mbc2 {
    pub fn new(rom: Vec<u8>) -> Self {
        Self {
            rom,
            ram: [0; 0x200],
            ram_enable: false,
            rom_bank: 1,
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
            0x0000..=0x1FFF => {
                if addr & 0x0100 == 0 {
                    self.ram_enable = is_ram_enable(value);
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

    pub fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enable {
            return 0xFF;
        }
        let idx = ((addr - RAM_BASE_ADDR) as usize) & 0x01FF;
        0xF0 | (self.ram[idx] & 0x0F)
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enable {
            return;
        }
        let idx = ((addr - RAM_BASE_ADDR) as usize) & 0x01FF;
        self.ram[idx] = value & 0x0F;
    }

    pub fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info("MBC2", self.rom_bank, 0, self.ram_enable, None)
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
        writer.write_bytes(&self.ram);
        writer.write_bool(self.ram_enable);
        writer.write_u64(self.rom_bank as u64);
    }

    pub(super) fn bess_mbc_writes(&self) -> Vec<(u16, u8)> {
        vec![
            (0x0000, if self.ram_enable { 0x0A } else { 0x00 }),
            (0x2100, (self.rom_bank & 0x0F) as u8),
        ]
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let mut ram = [0u8; 0x200];
        reader.read_exact(&mut ram)?;
        Ok(Self {
            rom: Vec::new(),
            ram,
            ram_enable: reader.read_bool()?,
            rom_bank: reader.read_u64()? as usize,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mbc2(n_banks: usize) -> Mbc2 {
        let mut rom = vec![0u8; n_banks * 0x4000];
        for bank in 0..n_banks {
            let start = bank * 0x4000;
            for byte in &mut rom[start..start + 0x4000] {
                *byte = bank as u8;
            }
        }
        Mbc2::new(rom)
    }

    #[test]
    fn default_bank_is_1() {
        let mbc = make_mbc2(4);
        assert_eq!(mbc.read_rom(0x4000), 1);
    }

    #[test]
    fn bank_0_corrected_to_1() {
        let mut mbc = make_mbc2(4);
        mbc.write_rom(0x2100, 0x00);
        assert_eq!(mbc.read_rom(0x4000), 1);
    }

    #[test]
    fn bank_switching() {
        let mut mbc = make_mbc2(8);
        mbc.write_rom(0x2100, 3);
        assert_eq!(mbc.read_rom(0x4000), 3);
    }

    #[test]
    fn bank_number_masked_to_4_bits() {
        let mut mbc = make_mbc2(16);
        mbc.write_rom(0x2100, 0xFF);
        assert_eq!(mbc.read_rom(0x4000), 15);
    }

    #[test]
    fn ram_enable_requires_bit8_clear() {
        let mut mbc = make_mbc2(4);

        mbc.write_rom(0x0000, 0x0A);
        mbc.write_ram(0xA000, 0x05);
        assert_eq!(mbc.read_ram(0xA000), 0xF5);

        mbc.write_rom(0x0000, 0x00);
        assert_eq!(mbc.read_ram(0xA000), 0xFF);
    }

    #[test]
    fn bank_select_requires_bit8_set() {
        let mut mbc = make_mbc2(8);
        mbc.write_rom(0x2100, 5);
        assert_eq!(mbc.read_rom(0x4000), 5);

        mbc.write_rom(0x2000, 3);
        assert_eq!(mbc.read_rom(0x4000), 5);
    }

    #[test]
    fn ram_is_4_bit() {
        let mut mbc = make_mbc2(4);
        mbc.write_rom(0x0000, 0x0A);
        mbc.write_ram(0xA000, 0xFF);
        assert_eq!(mbc.read_ram(0xA000), 0xFF);
        mbc.write_ram(0xA000, 0xA3);
        assert_eq!(mbc.read_ram(0xA000), 0xF3);
    }

    #[test]
    fn ram_mirrors_within_512_bytes() {
        let mut mbc = make_mbc2(4);
        mbc.write_rom(0x0000, 0x0A);
        mbc.write_ram(0xA000, 0x07);
        assert_eq!(mbc.read_ram(0xA200), mbc.read_ram(0xA000));
    }

    #[test]
    fn save_state_roundtrip() {
        let mut mbc = make_mbc2(4);
        mbc.write_rom(0x0000, 0x0A);
        mbc.write_rom(0x2100, 3);
        mbc.write_ram(0xA000, 0x09);

        let mut writer = StateWriter::new();
        mbc.write_state(&mut writer);
        let data = writer.into_bytes();
        let mut reader = StateReader::new(&data);
        let mut restored = Mbc2::read_state(&mut reader).unwrap();
        restored.restore_rom_bytes(mbc.rom.clone());

        assert_eq!(restored.read_rom(0x4000), 3);
        assert_eq!(restored.read_ram(0xA000), 0xF9);
    }
}

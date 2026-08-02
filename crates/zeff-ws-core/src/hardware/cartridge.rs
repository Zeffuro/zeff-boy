use anyhow::{Context, bail};

use super::constants::{LINEAR_ROM_WINDOW_SIZE, ROM_BANK_SIZE};

const FOOTER_SIZE: usize = 10;
const DEFAULT_OPEN_BUS: u8 = 0xFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinimumSystem {
    WonderSwan,
    WonderSwanColor,
    Unknown(u8),
}

impl MinimumSystem {
    fn from_byte(value: u8) -> Self {
        match value {
            0x00 => Self::WonderSwan,
            0x01 => Self::WonderSwanColor,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveKind {
    None,
    Sram8K,
    Sram32K,
    Sram128K,
    Sram256K,
    Eeprom128,
    Eeprom1K,
    Eeprom2K,
    Unknown(u8),
}

impl SaveKind {
    pub fn from_byte(value: u8) -> Self {
        match value {
            0x00 => Self::None,
            0x01 => Self::Sram8K,
            0x02 => Self::Sram32K,
            0x03 => Self::Sram128K,
            0x04 => Self::Sram256K,
            0x10 => Self::Eeprom128,
            0x20 => Self::Eeprom1K,
            0x50 => Self::Eeprom2K,
            other => Self::Unknown(other),
        }
    }

    pub fn size(self) -> usize {
        match self {
            Self::None | Self::Unknown(_) => 0,
            Self::Sram8K => 8 * 1024,
            Self::Sram32K => 32 * 1024,
            Self::Sram128K => 128 * 1024,
            Self::Sram256K => 256 * 1024,
            Self::Eeprom128 => 128,
            Self::Eeprom1K => 1024,
            Self::Eeprom2K => 2 * 1024,
        }
    }

    pub fn has_battery(self) -> bool {
        self.size() != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomSize {
    pub code: u8,
    pub declared_bytes: Option<usize>,
}

impl RomSize {
    fn from_code(code: u8) -> Self {
        let declared_bytes = match code {
            0x01 => Some(128 * 1024),
            0x02 => Some(512 * 1024),
            0x03 => Some(1024 * 1024),
            0x04 => Some(2 * 1024 * 1024),
            0x05 => Some(3 * 1024 * 1024),
            0x06 => Some(4 * 1024 * 1024),
            0x07 => Some(6 * 1024 * 1024),
            0x08 => Some(8 * 1024 * 1024),
            0x09 => Some(16 * 1024 * 1024),
            _ => None,
        };
        Self {
            code,
            declared_bytes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RomFooter {
    pub developer_id: u8,
    pub minimum_system: MinimumSystem,
    pub cartridge_id: u8,
    pub revision: u8,
    pub rom_size: RomSize,
    pub save_kind: SaveKind,
    pub flags: u8,
    pub rtc_present: bool,
    pub checksum: u16,
    pub computed_checksum: u16,
    pub checksum_valid: bool,
}

impl RomFooter {
    pub fn parse(rom: &[u8]) -> anyhow::Result<Self> {
        if rom.len() < FOOTER_SIZE {
            bail!("WonderSwan ROM is too small to contain a footer");
        }
        let base = rom.len() - FOOTER_SIZE;
        let checksum = u16::from_le_bytes([rom[base + 8], rom[base + 9]]);
        let computed_checksum = compute_footer_checksum(rom);
        Ok(Self {
            developer_id: rom[base],
            minimum_system: MinimumSystem::from_byte(rom[base + 1]),
            cartridge_id: rom[base + 2],
            revision: rom[base + 3],
            rom_size: RomSize::from_code(rom[base + 4]),
            save_kind: SaveKind::from_byte(rom[base + 5]),
            flags: rom[base + 6],
            rtc_present: rom[base + 7] != 0,
            checksum,
            computed_checksum,
            checksum_valid: checksum == computed_checksum,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Cartridge {
    rom: Vec<u8>,
    footer: RomFooter,
    save_data: Vec<u8>,
    bank0: u16,
    bank1: u16,
    linear_bank: u8,
}

impl Cartridge {
    pub fn load(rom_data: &[u8]) -> anyhow::Result<Self> {
        let footer = RomFooter::parse(rom_data).context("failed to parse WonderSwan ROM footer")?;
        let save_data = vec![0xFF; footer.save_kind.size()];
        let mut cart = Self {
            rom: rom_data.to_vec(),
            footer,
            save_data,
            bank0: 0,
            bank1: 0,
            linear_bank: 0,
        };
        cart.reset_banks();
        Ok(cart)
    }

    pub fn footer(&self) -> &RomFooter {
        &self.footer
    }

    pub fn rom(&self) -> &[u8] {
        &self.rom
    }

    pub fn has_battery(&self) -> bool {
        self.footer.save_kind.has_battery()
    }

    pub fn dump_battery_data(&self) -> Option<Vec<u8>> {
        self.has_battery().then(|| self.save_data.clone())
    }

    pub fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if !self.has_battery() {
            return Ok(());
        }
        if bytes.len() != self.save_data.len() {
            bail!(
                "WonderSwan save size mismatch: got {} bytes, expected {}",
                bytes.len(),
                self.save_data.len()
            );
        }
        self.save_data.copy_from_slice(bytes);
        Ok(())
    }

    pub fn reset_banks(&mut self) {
        let banks = self.rom_bank_count().max(1);
        self.bank0 = banks.saturating_sub(2) as u16;
        self.bank1 = banks.saturating_sub(1) as u16;
        self.linear_bank = self.last_linear_bank();
    }

    pub fn set_bank0(&mut self, value: u8) {
        self.bank0 = u16::from(value);
    }

    pub fn set_bank1(&mut self, value: u8) {
        self.bank1 = u16::from(value);
    }

    pub fn set_linear_bank(&mut self, value: u8) {
        self.linear_bank = value;
    }

    pub fn bank0(&self) -> u16 {
        self.bank0
    }

    pub fn bank1(&self) -> u16 {
        self.bank1
    }

    pub fn linear_bank(&self) -> u8 {
        self.linear_bank
    }

    pub fn save_data(&self) -> &[u8] {
        &self.save_data
    }

    pub(crate) fn save_data_mut(&mut self) -> &mut [u8] {
        &mut self.save_data
    }

    pub fn rom_read8(&self, addr: u32) -> u8 {
        let addr = (addr & 0x000F_FFFF) as usize;
        match addr {
            0x10000..=0x1FFFF => self.save_read8(addr - 0x10000),
            0x20000..=0x2FFFF => self.read_rom_bank(self.bank0, addr - 0x20000),
            0x30000..=0x3FFFF => self.read_rom_bank(self.bank1, addr - 0x30000),
            0x40000..=0xFFFFF => self.read_linear_window(addr - 0x40000),
            _ => DEFAULT_OPEN_BUS,
        }
    }

    pub fn rom_write8(&mut self, addr: u32, value: u8) {
        let addr = (addr & 0x000F_FFFF) as usize;
        if let 0x10000..=0x1FFFF = addr {
            self.save_write8(addr - 0x10000, value);
        }
    }

    fn rom_bank_count(&self) -> usize {
        self.rom.len().div_ceil(ROM_BANK_SIZE)
    }

    fn last_linear_bank(&self) -> u8 {
        if self.rom.len() <= LINEAR_ROM_WINDOW_SIZE {
            return 0;
        }
        ((self.rom.len() - LINEAR_ROM_WINDOW_SIZE) / (1024 * 1024)).min(u8::MAX as usize) as u8
    }

    fn read_rom_bank(&self, bank: u16, offset: usize) -> u8 {
        let rom_offset = usize::from(bank) * ROM_BANK_SIZE + offset;
        self.rom
            .get(rom_offset)
            .copied()
            .unwrap_or(DEFAULT_OPEN_BUS)
    }

    fn read_linear_window(&self, offset: usize) -> u8 {
        let preferred_base = usize::from(self.linear_bank) * 1024 * 1024;
        if let Some(value) = self.rom.get(preferred_base + 0x40000 + offset).copied() {
            return value;
        }

        if self.rom.len() >= LINEAR_ROM_WINDOW_SIZE {
            let base = self.rom.len() - LINEAR_ROM_WINDOW_SIZE;
            return self.rom.get(base + offset).copied().unwrap_or(DEFAULT_OPEN_BUS);
        }

        let leading_open_bus = LINEAR_ROM_WINDOW_SIZE - self.rom.len();
        if offset < leading_open_bus {
            DEFAULT_OPEN_BUS
        } else {
            self.rom
                .get(offset - leading_open_bus)
                .copied()
                .unwrap_or(DEFAULT_OPEN_BUS)
        }
    }

    fn save_read8(&self, offset: usize) -> u8 {
        if self.save_data.is_empty() {
            return DEFAULT_OPEN_BUS;
        }
        self.save_data[offset % self.save_data.len()]
    }

    fn save_write8(&mut self, offset: usize, value: u8) {
        if self.save_data.is_empty() {
            return;
        }
        let index = offset % self.save_data.len();
        self.save_data[index] = value;
    }
}

pub fn compute_footer_checksum(rom: &[u8]) -> u16 {
    let checksum_start = rom.len().saturating_sub(2);
    rom.iter()
        .take(checksum_start)
        .fold(0u16, |acc, &byte| acc.wrapping_add(u16::from(byte)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rom() -> Vec<u8> {
        let mut rom = vec![0xFF; 0x10000];
        let reset = rom.len() - 16;
        rom[reset] = 0x90;
        rom[reset + 1] = 0xF4;
        let footer = rom.len() - FOOTER_SIZE;
        rom[footer] = 0x01;
        rom[footer + 1] = 0x00;
        rom[footer + 2] = 0x23;
        rom[footer + 3] = 0x00;
        rom[footer + 4] = 0x01;
        rom[footer + 5] = 0x01;
        rom[footer + 6] = 0x00;
        rom[footer + 7] = 0x00;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        rom
    }

    #[test]
    fn parses_footer_from_end_of_rom() {
        let footer = RomFooter::parse(&test_rom()).unwrap();
        assert_eq!(footer.developer_id, 0x01);
        assert_eq!(footer.minimum_system, MinimumSystem::WonderSwan);
        assert_eq!(footer.cartridge_id, 0x23);
        assert_eq!(footer.rom_size.declared_bytes, Some(128 * 1024));
        assert_eq!(footer.save_kind, SaveKind::Sram8K);
        assert!(footer.checksum_valid);
    }

    #[test]
    fn loads_battery_save_with_declared_size() {
        let mut cart = Cartridge::load(&test_rom()).unwrap();
        assert_eq!(cart.dump_battery_data().unwrap().len(), 8 * 1024);
        assert!(cart.load_battery_data(&vec![0x11; 8 * 1024]).is_ok());
        assert!(cart.load_battery_data(&[0x11]).is_err());
    }

    #[test]
    fn maps_reset_fetch_to_end_of_small_rom() {
        let cart = Cartridge::load(&test_rom()).unwrap();
        assert_eq!(cart.rom_read8(0xFFFF0), 0x90);
        assert_eq!(cart.rom_read8(0xFFFF1), 0xF4);
    }

    #[test]
    fn banked_reads_follow_selected_bank() {
        let mut rom = test_rom();
        rom.resize(0x40000, 0xFF);
        rom[0x0000] = 0x12;
        rom[0x10000] = 0x34;
        rom[0x20000] = 0x56;
        let mut cart = Cartridge::load(&rom).unwrap();
        cart.set_bank0(0);
        cart.set_bank1(1);
        assert_eq!(cart.rom_read8(0x20000), 0x12);
        assert_eq!(cart.rom_read8(0x30000), 0x34);
        cart.set_bank0(2);
        assert_eq!(cart.rom_read8(0x20000), 0x56);
    }
}

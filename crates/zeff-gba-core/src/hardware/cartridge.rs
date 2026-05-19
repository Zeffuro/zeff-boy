use anyhow::{Context, bail};

use super::constants::{EEPROM_SIZE, FLASH_1M_SIZE, SRAM_SIZE};

const HEADER_END: usize = 0xC0;
const TITLE_START: usize = 0xA0;
const TITLE_END: usize = 0xAC;
const GAME_CODE_START: usize = 0xAC;
const GAME_CODE_END: usize = 0xB0;
const MAKER_CODE_START: usize = 0xB0;
const MAKER_CODE_END: usize = 0xB2;
const FIXED_VALUE_OFFSET: usize = 0xB2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupKind {
    None,
    Sram,
    Flash512,
    Flash1M,
    Eeprom,
}

impl BackupKind {
    pub fn size(self) -> usize {
        match self {
            Self::None => 0,
            Self::Sram | Self::Flash512 => SRAM_SIZE,
            Self::Flash1M => FLASH_1M_SIZE,
            Self::Eeprom => EEPROM_SIZE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RomHeader {
    pub title: String,
    pub game_code: String,
    pub maker_code: String,
    pub fixed_value: u8,
    pub complement_check: u8,
}

impl RomHeader {
    pub fn parse(rom: &[u8]) -> anyhow::Result<Self> {
        if rom.len() < HEADER_END {
            bail!("GBA ROM is too small to contain a header");
        }
        let fixed_value = rom[FIXED_VALUE_OFFSET];
        if fixed_value != 0x96 {
            bail!("invalid GBA header fixed value: {fixed_value:#04X}");
        }
        Ok(Self {
            title: ascii_field(&rom[TITLE_START..TITLE_END]),
            game_code: ascii_field(&rom[GAME_CODE_START..GAME_CODE_END]),
            maker_code: ascii_field(&rom[MAKER_CODE_START..MAKER_CODE_END]),
            fixed_value,
            complement_check: rom[0xBD],
        })
    }
}

#[derive(Clone, Debug)]
pub struct Cartridge {
    rom: Vec<u8>,
    header: RomHeader,
    backup_kind: BackupKind,
    backup: Vec<u8>,
}

impl Cartridge {
    pub fn load(rom_data: &[u8]) -> anyhow::Result<Self> {
        let header = RomHeader::parse(rom_data).context("failed to parse GBA ROM header")?;
        let backup_kind = detect_backup_kind(rom_data);
        Ok(Self {
            rom: rom_data.to_vec(),
            header,
            backup_kind,
            backup: vec![0xFF; backup_kind.size()],
        })
    }

    pub fn header(&self) -> &RomHeader {
        &self.header
    }

    pub fn rom(&self) -> &[u8] {
        &self.rom
    }

    pub fn backup_kind(&self) -> BackupKind {
        self.backup_kind
    }

    pub fn has_battery(&self) -> bool {
        self.backup_kind != BackupKind::None
    }

    pub fn dump_battery_data(&self) -> Option<Vec<u8>> {
        self.has_battery().then(|| self.backup.clone())
    }

    pub fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if !self.has_battery() {
            return Ok(());
        }
        if bytes.len() != self.backup.len() {
            bail!(
                "GBA save size mismatch: got {} bytes, expected {}",
                bytes.len(),
                self.backup.len()
            );
        }
        self.backup.copy_from_slice(bytes);
        Ok(())
    }

    pub fn rom_read8(&self, addr: u32) -> u8 {
        let Some(offset) = gba_rom_offset(addr) else {
            return 0xFF;
        };
        self.rom.get(offset).copied().unwrap_or(0xFF)
    }

    pub fn backup_read8(&self, addr: u32) -> u8 {
        if self.backup.is_empty() {
            return 0xFF;
        }
        self.backup[(addr as usize) & (self.backup.len() - 1)]
    }

    pub fn backup_write8(&mut self, addr: u32, value: u8) {
        if !self.backup.is_empty() {
            let index = (addr as usize) & (self.backup.len() - 1);
            self.backup[index] = value;
        }
    }
}

fn gba_rom_offset(addr: u32) -> Option<usize> {
    match addr {
        0x0800_0000..=0x09FF_FFFF => Some((addr - 0x0800_0000) as usize),
        0x0A00_0000..=0x0BFF_FFFF => Some((addr - 0x0A00_0000) as usize),
        0x0C00_0000..=0x0DFF_FFFF => Some((addr - 0x0C00_0000) as usize),
        _ => None,
    }
}

fn detect_backup_kind(rom: &[u8]) -> BackupKind {
    let haystack = rom.windows(8).take(0x20_0000);
    for window in haystack {
        if window.starts_with(b"FLASH1M_") {
            return BackupKind::Flash1M;
        }
        if window.starts_with(b"FLASH512") || window.starts_with(b"FLASH_V") {
            return BackupKind::Flash512;
        }
        if window.starts_with(b"SRAM_V") {
            return BackupKind::Sram;
        }
        if window.starts_with(b"EEPROM_V") {
            return BackupKind::Eeprom;
        }
    }
    BackupKind::None
}

fn ascii_field(bytes: &[u8]) -> String {
    bytes
        .iter()
        .copied()
        .take_while(|&b| b != 0)
        .filter(|&b| b.is_ascii_graphic() || b == b' ')
        .map(char::from)
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_rom() -> Vec<u8> {
        let mut rom = vec![0; 0xC0];
        rom[TITLE_START..TITLE_START + 4].copy_from_slice(b"TEST");
        rom[GAME_CODE_START..GAME_CODE_END].copy_from_slice(b"ABCD");
        rom[MAKER_CODE_START..MAKER_CODE_END].copy_from_slice(b"01");
        rom[FIXED_VALUE_OFFSET] = 0x96;
        rom
    }

    #[test]
    fn parses_minimal_header() {
        let rom = minimal_rom();
        let header = RomHeader::parse(&rom).unwrap();
        assert_eq!(header.title, "TEST");
        assert_eq!(header.game_code, "ABCD");
        assert_eq!(header.maker_code, "01");
    }

    #[test]
    fn rejects_bad_fixed_value() {
        let mut rom = minimal_rom();
        rom[FIXED_VALUE_OFFSET] = 0;
        assert!(RomHeader::parse(&rom).is_err());
    }

    #[test]
    fn detects_sram_marker() {
        let mut rom = minimal_rom();
        rom.extend_from_slice(b"SRAM_V113");
        let cart = Cartridge::load(&rom).unwrap();
        assert_eq!(cart.backup_kind(), BackupKind::Sram);
        assert_eq!(cart.dump_battery_data().unwrap().len(), SRAM_SIZE);
    }
}

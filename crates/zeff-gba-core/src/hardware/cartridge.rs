use anyhow::{Context, bail};
use std::cell::RefCell;

use super::constants::{EEPROM_SIZE, FLASH_1M_SIZE, SRAM_SIZE};

mod backup;

use backup::{EepromState, FlashState, detect_backup_kind};
const HEADER_END: usize = 0xC0;
const TITLE_START: usize = 0xA0;
const TITLE_END: usize = 0xAC;
const GAME_CODE_START: usize = 0xAC;
const GAME_CODE_END: usize = 0xB0;
const MAKER_CODE_START: usize = 0xB0;
const MAKER_CODE_END: usize = 0xB2;
const FIXED_VALUE_OFFSET: usize = 0xB2;
pub(crate) const EEPROM_WRITE_BUSY_CYCLES: u32 = 108_368;

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

impl BackupKind {
    fn is_flash(self) -> bool {
        matches!(self, Self::Flash512 | Self::Flash1M)
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
    flash: FlashState,
    eeprom: RefCell<EepromState>,
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
            flash: FlashState::default(),
            eeprom: RefCell::new(EepromState::default()),
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
        self.rom
            .get(offset)
            .copied()
            .unwrap_or_else(|| gamepak_open_bus_read8(addr))
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

fn gamepak_open_bus_read8(addr: u32) -> u8 {
    let halfword = ((addr >> 1) & 0xFFFF) as u16;
    halfword.to_le_bytes()[(addr & 1) as usize]
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

    #[test]
    fn detects_sram_f_marker() {
        let mut rom = minimal_rom();
        rom.extend_from_slice(b"SRAM_F_V102");
        let cart = Cartridge::load(&rom).unwrap();
        assert_eq!(cart.backup_kind(), BackupKind::Sram);
        assert_eq!(cart.dump_battery_data().unwrap().len(), SRAM_SIZE);
    }

    #[test]
    fn out_of_range_gamepak_rom_reads_address_open_bus_pattern() {
        let cart = Cartridge::load(&minimal_rom()).unwrap();

        assert_eq!(cart.rom_read8(0x0924_68AC), 0x56);
        assert_eq!(cart.rom_read8(0x0924_68AD), 0x34);
        assert_eq!(cart.rom_read8(0x0924_68AE), 0x57);
        assert_eq!(cart.rom_read8(0x0924_68AF), 0x34);
    }

    #[test]
    fn detects_backup_marker_after_first_two_mib() {
        let mut rom = minimal_rom();
        rom.resize(0x20_0000 + 16, 0);
        rom.extend_from_slice(b"FLASH1M_V103");

        let cart = Cartridge::load(&rom).unwrap();

        assert_eq!(cart.backup_kind(), BackupKind::Flash1M);
        assert_eq!(cart.dump_battery_data().unwrap().len(), FLASH_1M_SIZE);
    }

    fn flash1m_cart() -> Cartridge {
        let mut rom = minimal_rom();
        rom.extend_from_slice(b"FLASH1M_V103");
        Cartridge::load(&rom).unwrap()
    }

    fn flash512_cart() -> Cartridge {
        let mut rom = minimal_rom();
        rom.extend_from_slice(b"FLASH512_V131");
        Cartridge::load(&rom).unwrap()
    }

    fn eeprom_cart() -> Cartridge {
        let mut rom = minimal_rom();
        rom.extend_from_slice(b"EEPROM_V122");
        Cartridge::load(&rom).unwrap()
    }

    fn flash_unlock(cart: &mut Cartridge) {
        cart.backup_write8(0x0E00_5555, 0xAA);
        cart.backup_write8(0x0E00_2AAA, 0x55);
    }

    fn flash_command(cart: &mut Cartridge, command: u8) {
        flash_unlock(cart);
        cart.backup_write8(0x0E00_5555, command);
    }

    fn flash_program(cart: &mut Cartridge, addr: u32, value: u8) {
        flash_command(cart, 0xA0);
        cart.backup_write8(addr, value);
    }

    fn flash_bank(cart: &mut Cartridge, bank: u8) {
        flash_command(cart, 0xB0);
        cart.backup_write8(0x0E00_0000, bank);
    }

    #[test]
    fn flash1m_id_mode_returns_sanyo_id_and_exits_on_reset() {
        let mut cart = flash1m_cart();

        flash_command(&mut cart, 0x90);

        assert_eq!(cart.backup_read8(0x0E00_0000), 0x62);
        assert_eq!(cart.backup_read8(0x0E00_0001), 0x13);

        cart.backup_write8(0x0E00_0000, 0xF0);

        assert_eq!(cart.backup_read8(0x0E00_0000), 0xFF);
        assert_eq!(cart.backup_read8(0x0E00_0001), 0xFF);
    }

    #[test]
    fn flash512_id_mode_returns_panasonic_id() {
        let mut cart = flash512_cart();

        flash_command(&mut cart, 0x90);

        assert_eq!(cart.backup_read8(0x0E00_0000), 0x32);
        assert_eq!(cart.backup_read8(0x0E00_0001), 0x1B);
    }

    #[test]
    fn flash1m_programs_bytes_and_switches_banks() {
        let mut cart = flash1m_cart();

        flash_program(&mut cart, 0x0E00_1234, 0x5A);
        assert_eq!(cart.backup_read8(0x0E00_1234), 0x5A);

        flash_program(&mut cart, 0x0E00_1234, 0x0F);
        assert_eq!(cart.backup_read8(0x0E00_1234), 0x0A);

        flash_bank(&mut cart, 1);
        assert_eq!(cart.backup_read8(0x0E00_1234), 0xFF);

        flash_program(&mut cart, 0x0E00_1234, 0xC3);
        assert_eq!(cart.backup_read8(0x0E00_1234), 0xC3);

        flash_bank(&mut cart, 0);
        assert_eq!(cart.backup_read8(0x0E00_1234), 0x0A);
    }

    #[test]
    fn flash_sector_erase_only_clears_selected_sector() {
        let mut cart = flash1m_cart();

        flash_program(&mut cart, 0x0E00_0123, 0x11);
        flash_program(&mut cart, 0x0E00_1123, 0x22);

        flash_command(&mut cart, 0x80);
        flash_unlock(&mut cart);
        cart.backup_write8(0x0E00_0000, 0x30);

        assert_eq!(cart.backup_read8(0x0E00_0123), 0xFF);
        assert_eq!(cart.backup_read8(0x0E00_1123), 0x22);
    }

    #[test]
    fn eeprom_backup_does_not_respond_as_byte_addressable_sram() {
        let mut cart = eeprom_cart();

        cart.backup_write8(0x0E00_0000, 0x00);
        cart.backup_write8(0x0E00_0001, 0x12);

        assert_eq!(cart.backup_read8(0x0E00_0000), 0xFF);
        assert_eq!(cart.backup_read8(0x0E00_0001), 0xFF);
        assert_eq!(cart.dump_battery_data().unwrap(), vec![0xFF; EEPROM_SIZE]);
    }
}

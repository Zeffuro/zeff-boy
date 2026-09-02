use anyhow::{Context, bail};
use std::cell::RefCell;
use zeff_emu_common::save_ram::SaveRamKind;

use super::constants::{
    EEPROM_SIZE, FLASH_1M_SIZE, GAMEPAK0_END, GAMEPAK0_START, GAMEPAK1_END, GAMEPAK1_START,
    GAMEPAK2_END, GAMEPAK2_START, SRAM_SIZE,
};

mod backup;
mod rtc;

#[cfg(test)]
pub(crate) use backup::BACKUP_EXECUTION_STATE_SIZE;
use backup::{EepromState, FlashState, detect_backup_kind};
use rtc::RtcGpio;
pub use rtc::{RtcDateTime, RtcState};
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

    pub fn save_ram_kind(self) -> SaveRamKind {
        let size = self.size();
        if size == 0 {
            SaveRamKind::none()
        } else {
            SaveRamKind::known_battery_backed(size)
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
    rtc: Option<RtcGpio>,
}

impl Cartridge {
    pub fn load(rom_data: &[u8]) -> anyhow::Result<Self> {
        let header = RomHeader::parse(rom_data).context("failed to parse GBA ROM header")?;
        let backup_kind = detect_backup_kind(rom_data);
        let has_rtc = is_emerald_rtc(&header);
        Ok(Self {
            rom: rom_data.to_vec(),
            header,
            backup_kind,
            backup: vec![0xFF; backup_kind.size()],
            flash: FlashState::default(),
            eeprom: RefCell::new(EepromState::default()),
            rtc: has_rtc.then(RtcGpio::default),
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
        self.save_ram_kind().is_battery_backed()
    }

    pub fn save_ram_kind(&self) -> SaveRamKind {
        self.backup_kind.save_ram_kind()
    }

    pub fn has_rtc(&self) -> bool {
        self.rtc.is_some()
    }

    pub fn rtc_date_time(&self) -> Option<RtcDateTime> {
        self.rtc.as_ref().map(RtcGpio::date_time)
    }

    pub(crate) fn rtc_state(&self) -> Option<RtcState> {
        self.rtc.as_ref().map(RtcGpio::state)
    }

    pub fn set_rtc_date_time(&mut self, date_time: RtcDateTime) -> bool {
        let Some(rtc) = &mut self.rtc else {
            return false;
        };
        rtc.set_date_time(date_time);
        true
    }

    pub fn dump_battery_data(&self) -> Option<Vec<u8>> {
        self.has_battery().then(|| self.backup.clone())
    }

    pub(crate) fn dump_rtc_persistence_state(&self) -> Option<Vec<u8>> {
        self.rtc.as_ref().map(rtc::persistence::encode_state)
    }

    pub(crate) fn dump_complete_rtc_persistence(&self) -> Option<Vec<u8>> {
        let rtc = self.rtc.as_ref()?;
        let mut bytes = self.backup.clone();
        bytes.extend_from_slice(&rtc::persistence::encode_extension(rtc));
        Some(bytes)
    }

    pub(crate) fn load_complete_rtc_persistence(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let backup_len = self.backup.len();
        let extension = bytes
            .get(backup_len..)
            .ok_or_else(|| anyhow::anyhow!("truncated GBA RTC persistence"))?;
        let rtc = rtc::persistence::decode_extension(extension)?;
        anyhow::ensure!(self.rtc.is_some(), "GBA cartridge has no RTC");
        self.backup.copy_from_slice(&bytes[..backup_len]);
        self.rtc = Some(rtc);
        Ok(())
    }

    pub fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if !self.has_battery() {
            return Ok(());
        }
        let bytes = if self.rtc.is_some()
            && bytes.len() == self.backup.len() + rtc::persistence::EXTENSION_LEN
            && bytes[self.backup.len()..].starts_with(&rtc::persistence::MAGIC)
        {
            &bytes[..self.backup.len()]
        } else {
            bytes
        };
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
        if let Some(value) = self.rtc.as_ref().and_then(|rtc| rtc.read8(addr)) {
            return value;
        }
        let Some(offset) = gba_rom_offset(addr) else {
            return 0xFF;
        };
        self.rom
            .get(offset)
            .copied()
            .unwrap_or_else(|| gamepak_open_bus_read8(addr))
    }

    pub fn rom_read16(&self, addr: u32) -> u16 {
        let aligned = addr & !1;
        if self.rtc.is_none()
            && let Some(offset) = gba_rom_offset(aligned)
            && let Some(bytes) = self.rom.get(offset..offset + 2)
        {
            return u16::from_le_bytes([bytes[0], bytes[1]]);
        }
        u16::from_le_bytes([self.rom_read8(aligned), self.rom_read8(aligned + 1)])
    }

    pub fn rom_read32(&self, addr: u32) -> u32 {
        let aligned = addr & !3;
        if self.rtc.is_none()
            && let Some(offset) = gba_rom_offset(aligned)
            && let Some(bytes) = self.rom.get(offset..offset + 4)
        {
            return u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        u32::from_le_bytes([
            self.rom_read8(aligned),
            self.rom_read8(aligned + 1),
            self.rom_read8(aligned + 2),
            self.rom_read8(aligned + 3),
        ])
    }

    pub fn rom_write16(&mut self, addr: u32, value: u16) -> bool {
        self.rtc
            .as_mut()
            .is_some_and(|rtc| rtc.write16(addr, value))
    }

    pub(crate) fn write_rtc_state(&self, writer: &mut zeff_emu_common::save_state::StateWriter) {
        writer.write_bool(self.rtc.is_some());
        if let Some(rtc) = &self.rtc {
            rtc.write_state(writer);
        }
    }

    pub(crate) fn reset_rtc_state(&mut self) {
        if let Some(rtc) = &mut self.rtc {
            *rtc = RtcGpio::default();
        }
    }

    pub(crate) fn read_rtc_state(
        &mut self,
        reader: &mut zeff_emu_common::save_state::StateReader<'_>,
    ) -> anyhow::Result<()> {
        if !reader.read_bool()? {
            return Ok(());
        }
        let rtc = RtcGpio::read_state(reader)?;
        if let Some(current) = &mut self.rtc {
            *current = rtc;
        }
        Ok(())
    }
}

fn is_emerald_rtc(header: &RomHeader) -> bool {
    header.game_code.as_bytes().starts_with(b"BPE")
}

fn gba_rom_offset(addr: u32) -> Option<usize> {
    match addr {
        GAMEPAK0_START..=GAMEPAK0_END => Some((addr - GAMEPAK0_START) as usize),
        GAMEPAK1_START..=GAMEPAK1_END => Some((addr - GAMEPAK1_START) as usize),
        GAMEPAK2_START..=GAMEPAK2_END => Some((addr - GAMEPAK2_START) as usize),
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
        assert_eq!(
            cart.save_ram_kind(),
            SaveRamKind::known_battery_backed(SRAM_SIZE)
        );
        assert_eq!(cart.dump_battery_data().unwrap().len(), SRAM_SIZE);
    }

    #[test]
    fn no_backup_marker_reports_no_save_ram() {
        let cart = Cartridge::load(&minimal_rom()).unwrap();

        assert_eq!(cart.backup_kind(), BackupKind::None);
        assert_eq!(cart.save_ram_kind(), SaveRamKind::none());
        assert!(!cart.has_battery());
        assert_eq!(cart.dump_battery_data(), None);
    }

    #[test]
    fn emerald_game_code_enables_gpio_without_intercepting_disabled_reads() {
        let mut rom = minimal_rom();
        rom.resize(0xCA, 0);
        rom[GAME_CODE_START..GAME_CODE_END].copy_from_slice(b"BPEE");
        rom[0xC4] = 0xA5;
        let mut cart = Cartridge::load(&rom).unwrap();

        assert_eq!(cart.rom_read8(0x0800_00C4), 0xA5);
        assert!(cart.rom_write16(0x0800_00C8, 1));
        assert_eq!(cart.rom_read8(0x0800_00C8), 1);

        let mut other = Cartridge::load(&minimal_rom()).unwrap();
        assert!(!other.rom_write16(0x0800_00C8, 1));
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
    fn native_width_rom_reads_match_byte_reads() {
        let mut rom = minimal_rom();
        rom[0xB8..0xBC].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        let cart = Cartridge::load(&rom).unwrap();

        for addr in [0x0800_00B8, 0x0A00_00B8, 0x0924_68AC] {
            assert_eq!(
                cart.rom_read16(addr),
                u16::from_le_bytes([cart.rom_read8(addr), cart.rom_read8(addr + 1)])
            );
            assert_eq!(
                cart.rom_read32(addr),
                u32::from_le_bytes([
                    cart.rom_read8(addr),
                    cart.rom_read8(addr + 1),
                    cart.rom_read8(addr + 2),
                    cart.rom_read8(addr + 3),
                ])
            );
        }
    }

    #[test]
    fn native_width_rom_reads_preserve_rtc_gpio_intercepts() {
        let mut rom = minimal_rom();
        rom.resize(0xCA, 0);
        rom[GAME_CODE_START..GAME_CODE_END].copy_from_slice(b"BPEE");
        let mut cart = Cartridge::load(&rom).unwrap();
        assert!(cart.rom_write16(0x0800_00C8, 1));

        for addr in [0x0800_00C4, 0x0800_00C8] {
            assert_eq!(
                cart.rom_read16(addr),
                u16::from_le_bytes([cart.rom_read8(addr), cart.rom_read8(addr + 1)])
            );
            assert_eq!(
                cart.rom_read32(addr),
                u32::from_le_bytes([
                    cart.rom_read8(addr),
                    cart.rom_read8(addr + 1),
                    cart.rom_read8(addr + 2),
                    cart.rom_read8(addr + 3),
                ])
            );
        }
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

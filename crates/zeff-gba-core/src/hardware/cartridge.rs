use anyhow::{Context, bail};
use std::cell::RefCell;

use super::constants::{EEPROM_SIZE, FLASH_1M_SIZE, SRAM_SIZE};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FlashCommandState {
    #[default]
    Ready,
    Unlock1,
    Unlock2,
    Program,
    EraseSetup,
    EraseUnlock1,
    EraseUnlock2,
    BankSwitch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FlashState {
    command: FlashCommandState,
    id_mode: bool,
    bank: usize,
}

#[derive(Clone, Debug, Default)]
struct EepromState {
    command_bits: Vec<u8>,
    read_bits: Vec<u8>,
    read_index: usize,
    busy_cycles_remaining: u32,
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
        self.rom.get(offset).copied().unwrap_or(0xFF)
    }

    pub fn backup_read8(&self, addr: u32) -> u8 {
        if self.backup.is_empty() {
            return 0xFF;
        }
        if self.backup_kind == BackupKind::Eeprom {
            return 0xFF;
        }
        if self.backup_kind.is_flash() {
            return self.flash_read8(addr);
        }
        self.backup[(addr as usize) & (self.backup.len() - 1)]
    }

    pub fn backup_write8(&mut self, addr: u32, value: u8) {
        if self.backup.is_empty() {
            return;
        }
        if self.backup_kind == BackupKind::Eeprom {
            return;
        }
        if self.backup_kind.is_flash() {
            self.flash_write8(addr, value);
            return;
        }
        let index = (addr as usize) & (self.backup.len() - 1);
        self.backup[index] = value;
    }

    pub fn is_eeprom_access_addr(&self, addr: u32) -> bool {
        if self.backup_kind != BackupKind::Eeprom {
            return false;
        }
        if !matches!(addr, 0x0D00_0000..=0x0DFF_FFFF) {
            return false;
        }

        self.rom.len() <= 0x0100_0000 || addr >= 0x0DFF_FF00
    }

    pub fn eeprom_read16(&self, addr: u32) -> u16 {
        if !self.is_eeprom_access_addr(addr) {
            return 0xFFFF;
        }
        self.eeprom.borrow_mut().read16(&self.backup)
    }

    pub fn eeprom_write16(&mut self, addr: u32, value: u16) {
        if !self.is_eeprom_access_addr(addr) {
            return;
        }
        let _ = value;
    }

    pub fn eeprom_write_bits(&mut self, addr: u32, bits: &[u8]) {
        if !self.is_eeprom_access_addr(addr) {
            return;
        }
        self.eeprom.get_mut().write_bits(bits, &mut self.backup);
    }

    pub fn step_cycles(&mut self, cycles: u32) {
        if self.backup_kind != BackupKind::Eeprom {
            return;
        }
        self.eeprom.get_mut().step_cycles(cycles);
    }

    fn flash_read8(&self, addr: u32) -> u8 {
        let offset = flash_addr_offset(addr);
        if self.flash.id_mode {
            return match (self.backup_kind, offset) {
                (BackupKind::Flash512, 0) => 0x32,
                (BackupKind::Flash512, 1) => 0x1B,
                (BackupKind::Flash1M, 0) => 0x62,
                (BackupKind::Flash1M, 1) => 0x13,
                _ => 0xFF,
            };
        }

        let index = self.flash_index(offset);
        self.backup[index]
    }

    fn flash_write8(&mut self, addr: u32, value: u8) {
        let offset = flash_addr_offset(addr);
        match self.flash.command {
            FlashCommandState::Ready => {
                if offset == 0x5555 && value == 0xAA {
                    self.flash.command = FlashCommandState::Unlock1;
                } else if value == 0xF0 {
                    self.flash.id_mode = false;
                }
            }
            FlashCommandState::Unlock1 => {
                self.flash.command = if offset == 0x2AAA && value == 0x55 {
                    FlashCommandState::Unlock2
                } else {
                    FlashCommandState::Ready
                };
            }
            FlashCommandState::Unlock2 => {
                self.flash.command = FlashCommandState::Ready;
                match value {
                    0x90 if offset == 0x5555 => self.flash.id_mode = true,
                    0xF0 => self.flash.id_mode = false,
                    0xA0 if offset == 0x5555 => {
                        self.flash.command = FlashCommandState::Program;
                    }
                    0x80 if offset == 0x5555 => {
                        self.flash.command = FlashCommandState::EraseSetup;
                    }
                    0xB0 if offset == 0x5555 && self.backup_kind == BackupKind::Flash1M => {
                        self.flash.command = FlashCommandState::BankSwitch;
                    }
                    _ => {}
                }
            }
            FlashCommandState::Program => {
                let index = self.flash_index(offset);
                self.backup[index] &= value;
                self.flash.command = FlashCommandState::Ready;
            }
            FlashCommandState::EraseSetup => {
                self.flash.command = if offset == 0x5555 && value == 0xAA {
                    FlashCommandState::EraseUnlock1
                } else {
                    FlashCommandState::Ready
                };
            }
            FlashCommandState::EraseUnlock1 => {
                self.flash.command = if offset == 0x2AAA && value == 0x55 {
                    FlashCommandState::EraseUnlock2
                } else {
                    FlashCommandState::Ready
                };
            }
            FlashCommandState::EraseUnlock2 => {
                if offset == 0x5555 && value == 0x10 {
                    self.backup.fill(0xFF);
                } else if value == 0x30 {
                    self.erase_flash_sector(offset);
                }
                self.flash.command = FlashCommandState::Ready;
            }
            FlashCommandState::BankSwitch => {
                if offset == 0 {
                    self.flash.bank = usize::from(value & 1);
                }
                self.flash.command = FlashCommandState::Ready;
            }
        }
    }

    fn flash_index(&self, offset: usize) -> usize {
        let bank_base = if self.backup_kind == BackupKind::Flash1M {
            self.flash.bank * SRAM_SIZE
        } else {
            0
        };
        (bank_base + offset) & (self.backup.len() - 1)
    }

    fn erase_flash_sector(&mut self, offset: usize) {
        let start = self.flash_index(offset & !0x0FFF);
        let end = (start + 0x1000).min(self.backup.len());
        self.backup[start..end].fill(0xFF);
    }
}

impl EepromState {
    fn read16(&mut self, backup: &[u8]) -> u16 {
        if backup.is_empty() {
            return 0xFFFF;
        }
        if self.busy_cycles_remaining != 0 {
            return 0;
        }
        if self.read_index < self.read_bits.len() {
            let bit = self.read_bits[self.read_index];
            self.read_index += 1;
            return u16::from(bit & 1);
        }

        1
    }

    fn step_cycles(&mut self, cycles: u32) {
        self.busy_cycles_remaining = self.busy_cycles_remaining.saturating_sub(cycles);
    }

    fn write_bits(&mut self, bits: &[u8], backup: &mut [u8]) {
        if backup.is_empty() {
            return;
        }

        self.command_bits.clear();
        self.command_bits.extend(bits.iter().map(|bit| bit & 1));
        self.try_process_command(backup);
        self.command_bits.clear();
    }

    fn try_process_command(&mut self, backup: &mut [u8]) -> bool {
        match self.command_bits.as_slice() {
            [1, 1, ..] => self.try_process_read_command(backup),
            [1, 0, ..] => self.try_process_write_command(backup),
            bits if bits.len() >= 2 => true,
            _ => false,
        }
    }

    fn try_process_read_command(&mut self, backup: &[u8]) -> bool {
        for address_bits in [6, 14] {
            let command_len = eeprom_read_command_len(address_bits);
            if self.command_bits.len() != command_len {
                continue;
            }
            let page = eeprom_page_from_bits(&self.command_bits[2..2 + address_bits]);
            self.prepare_read_bits(backup, page);
            return true;
        }

        false
    }

    fn try_process_write_command(&mut self, backup: &mut [u8]) -> bool {
        for address_bits in [6, 14] {
            let command_len = eeprom_write_command_len(address_bits);
            if self.command_bits.len() != command_len {
                continue;
            }
            if self.command_bits[command_len - 1] != 0 {
                return true;
            }

            let page = eeprom_page_from_bits(&self.command_bits[2..2 + address_bits]);
            let data_start = 2 + address_bits;
            eeprom_write_page(
                backup,
                page,
                &self.command_bits[data_start..data_start + 64],
            );
            self.read_bits.clear();
            self.read_index = 0;
            self.busy_cycles_remaining = EEPROM_WRITE_BUSY_CYCLES;
            return true;
        }

        false
    }

    fn prepare_read_bits(&mut self, backup: &[u8], page: usize) {
        self.read_bits.clear();
        self.read_bits.extend_from_slice(&[0, 0, 0, 0]);

        let offset = eeprom_page_offset(backup, page);
        for byte in &backup[offset..offset + 8] {
            for bit in (0..8).rev() {
                self.read_bits.push((byte >> bit) & 1);
            }
        }
        self.read_index = 0;
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

fn flash_addr_offset(addr: u32) -> usize {
    (addr as usize) & (SRAM_SIZE - 1)
}

fn eeprom_read_command_len(address_bits: usize) -> usize {
    2 + address_bits + 1
}

fn eeprom_write_command_len(address_bits: usize) -> usize {
    2 + address_bits + 64 + 1
}

fn eeprom_page_from_bits(bits: &[u8]) -> usize {
    bits.iter()
        .fold(0usize, |value, &bit| (value << 1) | usize::from(bit & 1))
}

fn eeprom_page_offset(backup: &[u8], page: usize) -> usize {
    let page_count = (backup.len() / 8).max(1);
    (page & (page_count - 1)) * 8
}

fn eeprom_write_page(backup: &mut [u8], page: usize, bits: &[u8]) {
    let offset = eeprom_page_offset(backup, page);
    for (byte_index, byte) in backup[offset..offset + 8].iter_mut().enumerate() {
        let mut value = 0u8;
        for bit_index in 0..8 {
            value = (value << 1) | (bits[byte_index * 8 + bit_index] & 1);
        }
        *byte = value;
    }
}

fn detect_backup_kind(rom: &[u8]) -> BackupKind {
    let haystack = rom.windows(8);
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

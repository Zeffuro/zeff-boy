use super::{BackupKind, Cartridge, EEPROM_WRITE_BUSY_CYCLES};
use crate::hardware::constants::SRAM_SIZE;

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
pub(super) struct FlashState {
    command: FlashCommandState,
    id_mode: bool,
    bank: usize,
}

#[derive(Clone, Debug, Default)]
pub(super) struct EepromState {
    command_bits: Vec<u8>,
    read_bits: Vec<u8>,
    read_index: usize,
    busy_cycles_remaining: u32,
}

impl Cartridge {
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
        if let Some(rtc) = &mut self.rtc {
            rtc.step_cycles(cycles);
        }
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

pub(super) fn detect_backup_kind(rom: &[u8]) -> BackupKind {
    let haystack = rom.windows(8);
    for window in haystack {
        if window.starts_with(b"FLASH1M_") {
            return BackupKind::Flash1M;
        }
        if window.starts_with(b"FLASH512") || window.starts_with(b"FLASH_V") {
            return BackupKind::Flash512;
        }
        if window.starts_with(b"SRAM_V") || window.starts_with(b"SRAM_F") {
            return BackupKind::Sram;
        }
        if window.starts_with(b"EEPROM_V") {
            return BackupKind::Eeprom;
        }
    }
    BackupKind::None
}

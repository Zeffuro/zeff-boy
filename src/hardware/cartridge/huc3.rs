use super::CartridgeDebugInfo;
use super::{
    MAX_SAVE_RAM, build_debug_info, load_ram_into, read_banked_ram, read_banked_rom,
    read_fixed_rom, write_banked_ram,
};
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};

const FLAG_RAM_ENABLED: u8 = 0x0A;
const FLAG_COMMAND_MODE: u8 = 0x0B;
const FLAG_READ_MODE: u8 = 0x0C;
const FLAG_READY: u8 = 0x0D;
#[allow(dead_code)]
const FLAG_IR_MODE: u8 = 0x0E;

const RTC_CMD_READ_NIBBLE: u8 = 0x10;
const RTC_CMD_WRITE_NIBBLE: u8 = 0x30;
const RTC_CMD_MODE_SHIFT: u8 = 0x40;
const RTC_CMD_LATCH: u8 = 0x60;

const SECONDS_PER_DAY: u64 = 86_400;
const MINUTES_PER_DAY: u32 = 1_440;

pub(crate) struct HuC3 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    ram_enable: bool,
    rom_bank: usize,
    ram_bank: usize,
    ram_flag: u8,

    rtc_datetime: u32,
    rtc_writing_time: u32,
    rtc_clock_shift: u8,
    rtc_timer_read: bool,
    rtc_read_value: u8,

    rtc_base_time: u64,
}

impl HuC3 {
    pub(crate) fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        Self {
            rom,
            ram: vec![0; ram_size],
            ram_enable: false,
            rom_bank: 1,
            ram_bank: 0,
            ram_flag: 0,
            rtc_datetime: 0,
            rtc_writing_time: 0,
            rtc_clock_shift: 0,
            rtc_timer_read: false,
            rtc_read_value: 0,
            rtc_base_time: now_unix_seconds(),
        }
    }

    pub(crate) fn read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => read_fixed_rom(&self.rom, addr),
            0x4000..=0x7FFF => read_banked_rom(&self.rom, self.rom_bank, addr),
            _ => 0xFF,
        }
    }

    pub(crate) fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => {
                self.ram_enable = value == FLAG_RAM_ENABLED;
                self.ram_flag = value;
            }
            0x2000..=0x3FFF => {
                let bank = (value & 0x7F) as usize;
                self.rom_bank = if bank == 0 { 1 } else { bank };
            }
            0x4000..=0x5FFF => self.ram_bank = (value & 0x03) as usize,
            0x6000..=0x7FFF => { /* nothing */ }
            _ => {}
        }
    }

    pub(crate) fn read_ram(&self, addr: u16) -> u8 {
        match self.ram_flag {
            FLAG_READ_MODE => self.rtc_read_value,
            FLAG_READY => 1,
            FLAG_RAM_ENABLED => {
                if self.ram.is_empty() {
                    return 0xFF;
                }
                read_banked_ram(&self.ram, self.ram_bank, addr)
            }
            _ => 0xFF,
        }
    }

    pub(crate) fn write_ram(&mut self, addr: u16, value: u8) {
        if self.ram_flag == FLAG_COMMAND_MODE {
            self.handle_command(value);
            return;
        }

        if self.ram_flag == FLAG_RAM_ENABLED
            && !self.ram.is_empty() {
                write_banked_ram(&mut self.ram, self.ram_bank, addr, value);
            }
    }

    fn handle_command(&mut self, value: u8) {
        match value & 0xF0 {
            RTC_CMD_READ_NIBBLE => {
                if self.rtc_timer_read {
                    self.update_latch();
                    self.rtc_read_value =
                        ((self.rtc_datetime >> self.rtc_clock_shift) & 0x0F) as u8;
                    self.rtc_clock_shift += 4;
                    if self.rtc_clock_shift > 24 {
                        self.rtc_clock_shift = 0;
                    }
                }
            }
            RTC_CMD_WRITE_NIBBLE => {
                if !self.rtc_timer_read {
                    if self.rtc_clock_shift == 0 {
                        self.rtc_writing_time = 0;
                    }
                    if self.rtc_clock_shift < 24 {
                        self.rtc_writing_time |= ((value & 0x0F) as u32) << self.rtc_clock_shift;
                        self.rtc_clock_shift += 4;
                        if self.rtc_clock_shift == 24 {
                            self.commit_written_time();
                            self.rtc_timer_read = true;
                        }
                    }
                }
            }
            RTC_CMD_MODE_SHIFT => match value & 0x0F {
                0x0 => self.rtc_clock_shift = 0,
                0x3 => {
                    self.rtc_timer_read = false;
                    self.rtc_clock_shift = 0;
                }
                0x7 => {
                    self.rtc_timer_read = true;
                    self.rtc_clock_shift = 0;
                }
                _ => {}
            },
            RTC_CMD_LATCH => {
                self.rtc_timer_read = true;
            }
            _ => {}
        }
    }

    fn update_latch(&mut self) {
        let now = now_unix_seconds();
        let diff = now.saturating_sub(self.rtc_base_time);
        let minute = ((diff / 60) % MINUTES_PER_DAY as u64) as u32;
        let day = ((diff / SECONDS_PER_DAY) & 0xFFF) as u32;
        self.rtc_datetime = (day << 12) | minute;
    }

    fn commit_written_time(&mut self) {
        let now = now_unix_seconds();
        let minute = (self.rtc_writing_time & 0xFFF) % MINUTES_PER_DAY;
        let day = (self.rtc_writing_time >> 12) & 0xFFF;
        self.rtc_base_time = now - (minute as u64) * 60 - (day as u64) * SECONDS_PER_DAY;
    }

    pub(crate) fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub(crate) fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info("HuC3", self.rom_bank, self.ram_bank, self.ram_enable, None)
    }

    pub(crate) fn restore_rom_bytes(&mut self, rom: Vec<u8>) {
        self.rom = rom;
    }

    #[allow(dead_code)]
    pub(super) fn ram_bytes(&self) -> &[u8] {
        &self.ram
    }

    pub(super) fn sram_len(&self) -> usize {
        self.ram.len() + 8
    }

    pub(super) fn dump_sram(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.ram.len() + 8);
        bytes.extend_from_slice(&self.ram);
        bytes.extend_from_slice(&self.rtc_base_time.to_le_bytes());
        bytes
    }

    pub(super) fn load_sram(&mut self, bytes: &[u8]) {
        let ram_len = self.ram.len();
        load_ram_into(&mut self.ram, bytes);
        if bytes.len() >= ram_len + 8 {
            let mut ts = [0u8; 8];
            ts.copy_from_slice(&bytes[ram_len..ram_len + 8]);
            self.rtc_base_time = u64::from_le_bytes(ts);
        }
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_len(self.ram.len());
        writer.write_bytes(&self.ram);
        writer.write_bool(self.ram_enable);
        writer.write_u64(self.rom_bank as u64);
        writer.write_u64(self.ram_bank as u64);
        writer.write_u8(self.ram_flag);
        writer.write_u64(self.rtc_base_time);
        writer.write_u32(self.rtc_datetime);
        writer.write_u32(self.rtc_writing_time);
        writer.write_u8(self.rtc_clock_shift);
        writer.write_bool(self.rtc_timer_read);
        writer.write_u8(self.rtc_read_value);
    }

    pub(super) fn bess_mbc_writes(&self) -> Vec<(u16, u8)> {
        vec![
            (0x0000, if self.ram_enable { 0x0A } else { 0x00 }),
            (0x2000, self.rom_bank as u8),
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
            ram_flag: reader.read_u8()?,
            rtc_base_time: reader.read_u64()?,
            rtc_datetime: reader.read_u32()?,
            rtc_writing_time: reader.read_u32()?,
            rtc_clock_shift: reader.read_u8()?,
            rtc_timer_read: reader.read_bool()?,
            rtc_read_value: reader.read_u8()?,
        })
    }
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

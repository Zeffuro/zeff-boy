use super::CartridgeDebugInfo;
use super::rtc::{RTC_REG_COUNT, Rtc, T_CYCLES_PER_SECOND, sanitize_rtc_register};
use super::{
    MAX_SAVE_RAM, build_debug_info, is_ram_enable, load_ram_into, read_banked_ram, read_banked_rom,
    read_fixed_rom, write_banked_ram,
};
use crate::save_state::{StateReader, StateWriter, StateWriterGbExt};
use anyhow::Result;

mod sram;

pub struct Mbc3 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    has_rtc: bool,
    ram_enable: bool,
    rom_bank: usize,
    ram_or_rtc_select: u8,
    rtc_latch_write: u8,
    rtc: Rtc,
}

impl Mbc3 {
    pub fn new(rom: Vec<u8>, ram_size: usize, has_rtc: bool) -> Self {
        Self {
            rom,
            ram: vec![0; ram_size],
            has_rtc,
            ram_enable: false,
            rom_bank: 1,
            ram_or_rtc_select: 0,
            rtc_latch_write: 0,
            rtc: Rtc::new(),
        }
    }

    pub fn step(&mut self, t_cycles: u64) {
        if self.has_rtc {
            self.rtc.advance_cycles(t_cycles);
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
            0x2000..=0x3FFF => {
                let bank = (value & 0x7F) as usize;
                self.rom_bank = if bank == 0 { 1 } else { bank };
            }
            0x4000..=0x5FFF => self.ram_or_rtc_select = value,
            0x6000..=0x7FFF => {
                if self.has_rtc && self.rtc_latch_write == 0x00 && value == 0x01 {
                    self.rtc.latch();
                }
                self.rtc_latch_write = value;
            }
            _ => {}
        }
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enable {
            return 0xFF;
        }
        if self.has_rtc && (0x08..=0x0C).contains(&self.ram_or_rtc_select) {
            return self.rtc.read_latched(self.ram_or_rtc_select);
        }
        if self.ram.is_empty() {
            return 0xFF;
        }

        let bank = (self.ram_or_rtc_select & 0x03) as usize;
        read_banked_ram(&self.ram, bank, addr)
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enable {
            return;
        }

        if self.has_rtc && (0x08..=0x0C).contains(&self.ram_or_rtc_select) {
            self.rtc.write_internal(self.ram_or_rtc_select, value);
            return;
        }

        if self.ram.is_empty() {
            return;
        }

        let bank = (self.ram_or_rtc_select & 0x03) as usize;
        write_banked_ram(&mut self.ram, bank, addr, value);
    }

    pub fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info(
            "MBC3",
            self.rom_bank,
            (self.ram_or_rtc_select & 0x03) as usize,
            self.ram_enable,
            None,
        )
    }

    pub fn restore_rom_bytes(&mut self, rom: Vec<u8>) {
        self.rom = rom;
    }

    pub(super) fn load_ram_bytes(&mut self, bytes: &[u8]) {
        load_ram_into(&mut self.ram, bytes);
    }

    pub(super) fn ram_bytes(&self) -> &[u8] {
        &self.ram
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_len(self.ram.len());
        writer.write_bytes(&self.ram);
        writer.write_bool(self.has_rtc);
        writer.write_bool(self.ram_enable);
        writer.write_u64(self.rom_bank as u64);
        writer.write_u8(self.ram_or_rtc_select);
        writer.write_u8(self.rtc_latch_write);
        let rtc = &self.rtc;
        for value in rtc.internal {
            writer.write_u8(value);
        }
        for value in rtc.latched {
            writer.write_u8(value);
        }
        writer.write_u64(rtc.subsecond_cycles);
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let ram = reader.read_vec(MAX_SAVE_RAM)?;
        let has_rtc = reader.read_bool()?;
        let ram_enable = reader.read_bool()?;
        let rom_bank = reader.read_u64()? as usize;
        let ram_or_rtc_select = reader.read_u8()?;
        let rtc_latch_write = reader.read_u8()?;

        let mut internal = [0u8; RTC_REG_COUNT];
        let mut latched = [0u8; RTC_REG_COUNT];
        for (i, reg) in internal.iter_mut().enumerate() {
            *reg = sanitize_rtc_register(i, reader.read_u8()?);
        }
        for (i, reg) in latched.iter_mut().enumerate() {
            *reg = sanitize_rtc_register(i, reader.read_u8()?);
        }

        let raw_value = reader.read_u64()?;

        let subsecond_cycles = if raw_value < T_CYCLES_PER_SECOND {
            raw_value
        } else {
            0
        };

        Ok(Self {
            rom: Vec::new(),
            ram,
            has_rtc,
            ram_enable,
            rom_bank,
            ram_or_rtc_select,
            rtc_latch_write,
            rtc: Rtc {
                internal,
                latched,
                subsecond_cycles,
            },
        })
    }

    pub(super) fn bess_mbc_writes(&self) -> Vec<(u16, u8)> {
        vec![
            (0x0000, if self.ram_enable { 0x0A } else { 0x00 }),
            (0x2000, (self.rom_bank & 0x7F) as u8),
            (0x4000, self.ram_or_rtc_select),
        ]
    }

    pub(super) fn bess_rtc_data(&self) -> Option<([u8; 5], [u8; 5])> {
        if !self.has_rtc {
            return None;
        }
        Some((self.rtc.internal, self.rtc.latched))
    }

    pub(super) fn apply_bess_rtc(
        &mut self,
        current: [u8; 5],
        latched: [u8; 5],
        elapsed_seconds: u64,
    ) {
        if !self.has_rtc {
            return;
        }
        for (i, &val) in current.iter().enumerate() {
            self.rtc.internal[i] = sanitize_rtc_register(i, val);
        }
        for (i, &val) in latched.iter().enumerate() {
            self.rtc.latched[i] = sanitize_rtc_register(i, val);
        }
        self.rtc.subsecond_cycles = 0;
        self.rtc.catchup_seconds(elapsed_seconds);
    }
}

#[cfg(test)]
mod tests;

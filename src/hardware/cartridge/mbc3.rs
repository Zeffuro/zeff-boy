use super::CartridgeDebugInfo;
use super::rtc::{RTC_REG_COUNT, Rtc, T_CYCLES_PER_SECOND, sanitize_rtc_register};
use super::{
    MAX_SAVE_RAM, build_debug_info, is_ram_enable, load_ram_into, read_banked_ram, read_banked_rom,
    read_fixed_rom, write_banked_ram,
};
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

mod sram;

pub(crate) struct Mbc3 {
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
    pub(crate) fn new(rom: Vec<u8>, ram_size: usize, has_rtc: bool) -> Self {
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

    pub(crate) fn step(&mut self, t_cycles: u64) {
        if self.has_rtc {
            self.rtc.advance_cycles(t_cycles);
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

    pub(crate) fn read_ram(&self, addr: u16) -> u8 {
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

    pub(crate) fn write_ram(&mut self, addr: u16, value: u8) {
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

    pub(crate) fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub(crate) fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info(
            "MBC3",
            self.rom_bank,
            (self.ram_or_rtc_select & 0x03) as usize,
            self.ram_enable,
            None,
        )
    }

    pub(crate) fn restore_rom_bytes(&mut self, rom: Vec<u8>) {
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
mod tests {
    use super::*;
    use crate::hardware::cartridge::rtc::{
        RTC_DAY_HIGH, RTC_DAY_LOW, RTC_DH_CARRY_BIT, RTC_DH_DAY_HIGH_BIT, RTC_HOURS, RTC_MINUTES,
        RTC_SECONDS,
    };

    #[test]
    fn rtc_registers_are_latched_on_00_to_01_transition() {
        let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x2000, true);
        mbc3.write_rom(0x0000, 0x0A);

        mbc3.write_rom(0x4000, 0x08);
        mbc3.write_ram(0xA000, 12);
        mbc3.write_rom(0x4000, 0x09);
        mbc3.write_ram(0xA000, 34);
        mbc3.write_rom(0x4000, 0x0A);
        mbc3.write_ram(0xA000, 56);

        mbc3.write_rom(0x6000, 0x00);
        mbc3.write_rom(0x6000, 0x01);

        mbc3.write_rom(0x4000, 0x08);
        assert_eq!(mbc3.read_ram(0xA000), 12);
        mbc3.write_rom(0x4000, 0x09);
        assert_eq!(mbc3.read_ram(0xA000), 34);
        mbc3.write_rom(0x4000, 0x0A);
        assert_eq!(mbc3.read_ram(0xA000), 24);
    }

    #[test]
    fn rtc_overflow_sets_carry_and_wraps_day_counter() {
        let rtc = &mut Rtc {
            internal: [59, 59, 23, 0xFF, 0x01],
            latched: [0; RTC_REG_COUNT],
            subsecond_cycles: 0,
        };
        rtc.advance_cycles(T_CYCLES_PER_SECOND);

        assert_eq!(rtc.internal[RTC_SECONDS], 0);
        assert_eq!(rtc.internal[RTC_MINUTES], 0);
        assert_eq!(rtc.internal[RTC_HOURS], 0);
        assert_eq!(rtc.internal[RTC_DAY_LOW], 0);
        assert_eq!(rtc.internal[RTC_DAY_HIGH] & RTC_DH_DAY_HIGH_BIT, 0);
        assert_ne!(rtc.internal[RTC_DAY_HIGH] & RTC_DH_CARRY_BIT, 0);
    }

    #[test]
    fn rtc_sram_footer_44_byte_format_is_loaded() {
        let mut mbc3 = Mbc3::new(vec![0; 0x8000], 2, true);
        let mut bytes = vec![0xAA, 0xBB];
        let regs: [u8; 10] = [1, 2, 3, 4, 0x41, 5, 6, 7, 8, 0x80];
        for reg in regs {
            bytes.extend_from_slice(&(reg as u32).to_le_bytes());
        }
        bytes.extend_from_slice(&1234u32.to_le_bytes());

        mbc3.load_sram(&bytes);

        assert_eq!(mbc3.ram, vec![0xAA, 0xBB]);
        let rtc = &mbc3.rtc;
        assert_eq!(rtc.internal, [1, 2, 3, 4, 0x41]);
        assert_eq!(rtc.latched, [5, 6, 7, 8, 0x80]);
    }

    #[test]
    fn rtc_tick_based_on_t_cycles() {
        let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x2000, true);
        mbc3.write_rom(0x0000, 0x0A);
        mbc3.write_rom(0x4000, 0x08);
        mbc3.write_ram(0xA000, 0); // seconds = 0

        // Advance just under 1 second - should not tick
        mbc3.step(T_CYCLES_PER_SECOND - 1);
        mbc3.write_rom(0x6000, 0x00);
        mbc3.write_rom(0x6000, 0x01);
        assert_eq!(mbc3.read_ram(0xA000), 0);

        // Advance 1 more cycle - should tick to 1
        mbc3.step(1);
        mbc3.write_rom(0x6000, 0x00);
        mbc3.write_rom(0x6000, 0x01);
        assert_eq!(mbc3.read_ram(0xA000), 1);
    }

    #[test]
    fn rtc_subsecond_preserved_on_non_seconds_write() {
        let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x2000, true);
        mbc3.write_rom(0x0000, 0x0A);

        // Advance 500ms worth of cycles
        let half_second = T_CYCLES_PER_SECOND / 2;
        mbc3.step(half_second);

        // Write to minutes register (should preserve sub-second)
        mbc3.write_rom(0x4000, 0x09);
        mbc3.write_ram(0xA000, 5);

        // Advance another 500ms - should now tick
        mbc3.step(half_second);
        mbc3.write_rom(0x6000, 0x00);
        mbc3.write_rom(0x6000, 0x01);
        mbc3.write_rom(0x4000, 0x08);
        assert_eq!(mbc3.read_ram(0xA000), 1);
    }

    #[test]
    fn rtc_subsecond_reset_on_seconds_write() {
        let mut mbc3 = Mbc3::new(vec![0; 0x8000], 0x2000, true);
        mbc3.write_rom(0x0000, 0x0A);

        // Advance 500ms worth of cycles
        let half_second = T_CYCLES_PER_SECOND / 2;
        mbc3.step(half_second);

        // Write to seconds register (should reset sub-second)
        mbc3.write_rom(0x4000, 0x08);
        mbc3.write_ram(0xA000, 10);

        // Advance another 500ms - should NOT tick (sub-second was reset)
        mbc3.step(half_second);
        mbc3.write_rom(0x6000, 0x00);
        mbc3.write_rom(0x6000, 0x01);
        assert_eq!(mbc3.read_ram(0xA000), 10);

        // Need full 1s from the seconds write to tick
        mbc3.step(half_second);
        mbc3.write_rom(0x6000, 0x00);
        mbc3.write_rom(0x6000, 0x01);
        assert_eq!(mbc3.read_ram(0xA000), 11);
    }
}

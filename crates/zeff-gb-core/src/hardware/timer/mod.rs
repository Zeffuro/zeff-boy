use crate::hardware::types::hardware_mode::HardwareMode;
use crate::hardware::types::TimerClock;
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;
use std::fmt;

pub(super) struct Timer {
    div: u8,
    tima: u8,
    tma: u8,
    tac: u8,
    sys_counter: u16,
    mode: HardwareMode,
    prev_bit: bool,
    overflow_delay: u8,
}

impl fmt::Debug for Timer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Timer")
            .field("div", &format_args!("{:#04X}", self.div))
            .field("tima", &format_args!("{:#04X}", self.tima))
            .field("tma", &format_args!("{:#04X}", self.tma))
            .field("tac", &format_args!("{:#04X}", self.tac))
            .field("sys_counter", &self.sys_counter)
            .field("mode", &self.mode)
            .finish()
    }
}

impl Timer {
    pub(super) fn new() -> Self {
        Self {
            div: 0xAD,
            tima: 0,
            tma: 0,
            tac: 0,
            sys_counter: 0,
            mode: HardwareMode::DMG,
            prev_bit: false,
            overflow_delay: 0,
        }
    }

    pub(super) fn apply_bess_div(&mut self, div: u8) {
        self.div = div;
        self.sys_counter = (div as u16) << 8;
        self.prev_bit = false;
        self.overflow_delay = 0;
    }

    pub(super) fn div(&self) -> u8 {
        self.div
    }

    pub(super) fn tima(&self) -> u8 {
        self.tima
    }

    pub(super) fn tma(&self) -> u8 {
        self.tma
    }

    pub(super) fn tac(&self) -> u8 {
        self.tac
    }


    pub(super) fn set_mode(&mut self, mode: HardwareMode) {
        self.mode = mode;
    }

    fn timer_bit_mask(&self) -> u16 {
        let clock = TimerClock::from_bits(self.tac);
        let freq = clock.increment_cycles(self.mode);
        (freq >> 1) as u16
    }

    fn timer_tick_bit(&self) -> bool {
        let enabled = self.tac & 0x04 != 0;
        let bit_high = self.sys_counter & self.timer_bit_mask() != 0;
        enabled && bit_high
    }

    pub(super) fn reset_div(&mut self) {
        let old_bit = self.timer_tick_bit();
        self.sys_counter = 0;
        self.div = 0;
        let new_bit = self.timer_tick_bit();
        if old_bit && !new_bit {
            self.increment_tima();
        }
        self.prev_bit = new_bit;
    }

    pub(super) fn write_tima(&mut self, value: u8) {
        self.overflow_delay = 0;
        self.tima = value;
    }

    pub(super) fn write_tma(&mut self, value: u8) {
        self.tma = value;
    }

    pub(super) fn write_tac(&mut self, value: u8) {
        let old_bit = self.timer_tick_bit();
        self.tac = value;
        let new_bit = self.timer_tick_bit();
        if old_bit && !new_bit {
            self.increment_tima();
        }
        self.prev_bit = new_bit;
    }

    pub(super) fn set_tima_raw(&mut self, value: u8) {
        self.tima = value;
    }

    pub(super) fn set_tma_raw(&mut self, value: u8) {
        self.tma = value;
    }

    pub(super) fn set_tac_raw(&mut self, value: u8) {
        self.tac = value;
    }

    fn increment_tima(&mut self) {
        let (new_tima, overflow) = self.tima.overflowing_add(1);
        if overflow {
            self.tima = 0;
            self.overflow_delay = 4;
        } else {
            self.tima = new_tima;
        }
    }

    pub(super) fn step(&mut self, cycles: u64) -> bool {
        let mut interrupt = false;
        let mask = self.timer_bit_mask();
        let enabled = self.tac & 0x04 != 0;

        for _ in 0..cycles {
            if self.overflow_delay > 0 {
                self.overflow_delay -= 1;
                if self.overflow_delay == 0 {
                    self.tima = self.tma;
                    interrupt = true;
                }
            }

            self.sys_counter = self.sys_counter.wrapping_add(1);
            self.div = (self.sys_counter >> 8) as u8;

            let new_bit = enabled && (self.sys_counter & mask != 0);
            if self.prev_bit && !new_bit {
                self.increment_tima();
            }
            self.prev_bit = new_bit;
        }

        interrupt
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.div);
        writer.write_u8(self.tima);
        writer.write_u8(self.tma);
        writer.write_u8(self.tac);
        writer.write_u16(self.sys_counter);
        writer.write_hardware_mode(self.mode);
        writer.write_bool(self.prev_bit);
        writer.write_u8(self.overflow_delay);
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            div: reader.read_u8()?,
            tima: reader.read_u8()?,
            tma: reader.read_u8()?,
            tac: reader.read_u8()?,
            sys_counter: reader.read_u16()?,
            mode: reader.read_hardware_mode()?,
            prev_bit: reader.read_bool()?,
            overflow_delay: reader.read_u8()?,
        })
    }
}

#[cfg(test)]
mod tests;

use crate::hardware::types::TimerClock;
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::save_state::{StateReader, StateReaderGbExt, StateWriter, StateWriterGbExt};
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
    div_apu_events: u8,
    div_apu_secondary_events: u8,
    overflow_delay: u8,
    reload_during_step: bool,
    cpu_interrupt_pending_before_if: bool,
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
            div_apu_events: 0,
            div_apu_secondary_events: 0,
            overflow_delay: 0,
            reload_during_step: false,
            cpu_interrupt_pending_before_if: false,
        }
    }

    pub(super) fn apply_bess_div(&mut self, div: u8) {
        self.div = div;
        self.sys_counter = (div as u16) << 8;
        self.prev_bit = false;
        self.div_apu_events = 0;
        self.div_apu_secondary_events = 0;
        self.overflow_delay = 0;
        self.reload_during_step = false;
        self.cpu_interrupt_pending_before_if = false;
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
        self.tac | 0xF8
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

    fn div_apu_bit_mask(&self) -> u16 {
        match self.mode {
            HardwareMode::CGBDouble => 1 << 13,
            _ => 1 << 12,
        }
    }

    pub(super) fn div_apu_bit(&self) -> bool {
        self.sys_counter & self.div_apu_bit_mask() != 0
    }

    pub(super) fn drain_div_apu_events(&mut self) -> u8 {
        let events = self.div_apu_events;
        self.div_apu_events = 0;
        events
    }

    pub(super) fn drain_div_apu_secondary_events(&mut self) -> u8 {
        let events = self.div_apu_secondary_events;
        self.div_apu_secondary_events = 0;
        events
    }

    pub(super) fn drain_cpu_interrupt_pending_before_if(&mut self) -> bool {
        let pending = self.cpu_interrupt_pending_before_if;
        self.cpu_interrupt_pending_before_if = false;
        pending
    }

    pub(super) fn reset_div(&mut self) -> bool {
        let old_bit = self.timer_tick_bit();
        let old_apu_bit = self.div_apu_bit();
        self.sys_counter = 0;
        self.div = 0;
        let new_bit = self.timer_tick_bit();
        let new_apu_bit = self.div_apu_bit();
        if old_apu_bit && !new_apu_bit {
            self.div_apu_events = self.div_apu_events.saturating_add(1);
        }
        let overflowed = old_bit && !new_bit && self.increment_tima();
        self.prev_bit = new_bit;
        overflowed
    }

    pub(super) fn reset_div_after_cpu_write_cycle(&mut self) -> bool {
        let overflowed = self.reset_div();
        self.finish_register_write_overflow(overflowed)
    }

    pub(super) fn write_tima(&mut self, value: u8) {
        if self.reload_during_step {
            return;
        }

        self.overflow_delay = 0;
        self.cpu_interrupt_pending_before_if = false;
        self.tima = value;
    }

    pub(super) fn write_tma(&mut self, value: u8) {
        self.tma = value;
        if self.reload_during_step {
            self.tima = value;
        }
    }

    pub(super) fn write_tac(&mut self, value: u8) -> bool {
        let old_bit = self.timer_tick_bit();
        self.tac = value;
        let new_bit = self.timer_tick_bit();
        let overflowed = old_bit && !new_bit && self.increment_tima();
        self.prev_bit = new_bit;
        overflowed
    }

    pub(super) fn write_tac_after_cpu_write_cycle(&mut self, value: u8) -> bool {
        let overflowed = self.write_tac(value);
        self.finish_register_write_overflow(overflowed)
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

    pub(super) fn set_divider_counter_raw(&mut self, value: u16) {
        self.sys_counter = value;
        self.div = (value >> 8) as u8;
        self.prev_bit = self.timer_tick_bit();
        self.div_apu_events = 0;
        self.div_apu_secondary_events = 0;
    }

    fn increment_tima(&mut self) -> bool {
        let (new_tima, overflow) = self.tima.overflowing_add(1);
        if overflow {
            self.tima = 0;
            self.overflow_delay = 4;
            self.cpu_interrupt_pending_before_if = true;
            true
        } else {
            self.tima = new_tima;
            false
        }
    }

    fn finish_register_write_overflow(&mut self, overflowed: bool) -> bool {
        if !overflowed {
            return false;
        }

        self.overflow_delay = 0;
        self.cpu_interrupt_pending_before_if = false;
        self.tima = self.tma;
        self.reload_during_step = true;
        true
    }

    pub(super) fn step(&mut self, cycles: u64) -> bool {
        let mut interrupt = false;
        let mask = self.timer_bit_mask();
        let enabled = self.tac & 0x04 != 0;
        self.reload_during_step = false;

        for _ in 0..cycles {
            let old_apu_bit = self.div_apu_bit();
            if self.overflow_delay > 0 {
                self.overflow_delay -= 1;
                if self.overflow_delay == 0 {
                    self.tima = self.tma;
                    self.reload_during_step = true;
                    interrupt = true;
                }
            }

            self.sys_counter = self.sys_counter.wrapping_add(1);
            self.div = (self.sys_counter >> 8) as u8;
            let new_apu_bit = self.div_apu_bit();
            if old_apu_bit && !new_apu_bit {
                self.div_apu_events = self.div_apu_events.saturating_add(1);
            } else if !old_apu_bit && new_apu_bit {
                self.div_apu_secondary_events = self.div_apu_secondary_events.saturating_add(1);
            }

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
            div_apu_events: 0,
            div_apu_secondary_events: 0,
            overflow_delay: reader.read_u8()?,
            reload_during_step: false,
            cpu_interrupt_pending_before_if: false,
        })
    }
}

#[cfg(test)]
mod tests;

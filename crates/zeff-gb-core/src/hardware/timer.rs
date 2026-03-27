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
mod tests {
    use super::*;

    fn make_timer() -> Timer {
        let mut t = Timer::new();
        t.set_mode(HardwareMode::DMG);
        t
    }

    #[test]
    fn div_increments_every_256_t_cycles() {
        let mut t = make_timer();
        t.reset_div();
        assert_eq!(t.div(), 0);

        t.step(255);
        assert_eq!(t.div(), 0);

        t.step(1);
        assert_eq!(t.div(), 1);

        t.step(256);
        assert_eq!(t.div(), 2);
    }

    #[test]
    fn reset_div_clears_sys_counter() {
        let mut t = make_timer();
        t.step(512);
        assert!(t.div() > 0);

        t.reset_div();
        assert_eq!(t.div(), 0);
    }

    #[test]
    fn tima_does_not_increment_when_disabled() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x00);
        t.step(1024);
        assert_eq!(t.tima(), 0);
    }

    #[test]
    fn tima_increments_at_clock_rate_div4() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x05);
        t.step(15);
        assert_eq!(t.tima(), 0);
        t.step(1);
        assert_eq!(t.tima(), 1);
    }

    #[test]
    fn tima_overflow_reloads_from_tma_and_fires_interrupt() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x05);
        t.set_tima_raw(0xFF);
        t.set_tma_raw(0x42);
        let irq = t.step(20);
        assert_eq!(t.tima(), 0x42);
        assert!(irq, "timer overflow should generate interrupt after 4-cycle delay");
    }

    #[test]
    fn tima_overflow_reads_zero_during_delay() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x05);
        t.set_tima_raw(0xFF);
        t.set_tma_raw(0x10);
        for _ in 0..15 {
            assert!(!t.step(1));
        }
        assert_eq!(t.tima(), 0xFF);
        assert!(!t.step(1));
        assert_eq!(t.tima(), 0x00, "TIMA should read 0 during overflow delay");
        assert!(!t.step(1));
        assert_eq!(t.tima(), 0x00);
        assert!(!t.step(1));
        assert_eq!(t.tima(), 0x00);
        assert!(!t.step(1));
        assert_eq!(t.tima(), 0x00);
        let irq = t.step(1);
        assert!(irq, "interrupt should fire after 4-cycle delay");
        assert_eq!(t.tima(), 0x10, "TIMA should be reloaded from TMA");
    }

    #[test]
    fn write_tima_cancels_pending_overflow() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x05);
        t.set_tima_raw(0xFF);
        t.set_tma_raw(0x20);
        t.step(16); // Trigger overflow — TIMA=0, delay=4
        assert_eq!(t.tima(), 0x00, "TIMA should be 0 during delay");
        t.write_tima(0x50); // Cancel the pending reload
        assert_eq!(t.tima(), 0x50);
        let irq = t.step(1);
        assert!(!irq, "interrupt should be cancelled by TIMA write");
        assert_ne!(t.tima(), 0x20, "TMA reload should be cancelled");
    }

    #[test]
    fn tac_glitch_falling_edge_increments_tima() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x05);
        t.step(8);
        assert_eq!(t.tima(), 0);
        let tima_before = t.tima();
        t.write_tac(0x00);
        assert_eq!(t.tima(), tima_before + 1);
    }

    #[test]
    fn reset_div_falling_edge_increments_tima() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x04);
        t.step(512);
        assert_eq!(t.tima(), 0);
        let tima_before = t.tima();
        t.reset_div();
        assert_eq!(t.tima(), tima_before + 1);
    }

    #[test]
    fn div_wraps_around_at_255() {
        let mut t = make_timer();
        t.reset_div();
        t.step(255 * 256);
        assert_eq!(t.div(), 255);
        t.step(256);
        assert_eq!(t.div(), 0);
    }

    #[test]
    fn save_state_roundtrip() {
        let mut t = make_timer();
        t.write_tac(0x07);
        t.set_tma_raw(0x42);
        t.step(100);

        let mut writer = StateWriter::new();
        t.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = StateReader::new(&bytes);
        let restored = Timer::read_state(&mut reader).expect("restore should succeed");

        assert_eq!(restored.div(), t.div());
        assert_eq!(restored.tima(), t.tima());
        assert_eq!(restored.tma(), t.tma());
        assert_eq!(restored.tac(), t.tac());
    }
    
    #[test]
    fn tac_write_during_pending_overflow_does_not_cancel() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x05);
        t.set_tima_raw(0xFF);
        t.set_tma_raw(0x30);
        t.step(16);
        assert_eq!(t.tima(), 0x00, "TIMA should be 0 during overflow delay");
        t.write_tac(0x07);
        let irq = t.step(4);
        assert!(irq, "overflow interrupt should still fire after TAC write");
        assert_eq!(t.tima(), 0x30, "TMA reload should still happen after TAC write");
    }

    #[test]
    fn div_reset_during_pending_overflow_does_not_cancel() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x05);
        t.set_tima_raw(0xFF);
        t.set_tma_raw(0x25);
        t.step(16);
        assert_eq!(t.tima(), 0x00, "TIMA should be 0 during overflow delay");
        t.reset_div();
        let irq = t.step(4);
        assert!(irq, "overflow interrupt should still fire after DIV reset");
        assert_eq!(t.tima(), 0x25, "TMA reload should happen after DIV reset");
    }
    
    #[test]
    fn overflow_with_tma_fe_cascades_on_second_tick() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x05);
        t.set_tima_raw(0xFF);
        t.set_tma_raw(0xFE);
        t.step(16);
        assert_eq!(t.tima(), 0x00, "TIMA should read 0 during delay");
        
        let irq1 = t.step(4);
        assert!(irq1, "first overflow interrupt");
        assert_eq!(t.tima(), 0xFE, "TIMA should reload to TMA (0xFE)");
        t.step(12);
        assert_eq!(t.tima(), 0xFF, "TIMA should be 0xFF after one increment");
        
        let irq2 = t.step(20);
        assert!(irq2, "second overflow interrupt should fire");
        assert_eq!(t.tima(), 0xFE, "TIMA should reload to TMA (0xFE) again");
    }

    #[test]
    fn tma_write_during_overflow_delay_uses_new_value() {
        let mut t = make_timer();
        t.reset_div();
        t.write_tac(0x05);
        t.set_tima_raw(0xFF);
        t.set_tma_raw(0x10);
        t.step(16);
        assert_eq!(t.tima(), 0x00, "TIMA should be 0 during delay");
        t.write_tma(0x42);
        
        let irq = t.step(4);
        assert!(irq, "interrupt should fire");
        assert_eq!(t.tima(), 0x42, "TIMA should reload from new TMA value");
    }
}

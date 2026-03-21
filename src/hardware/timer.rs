use crate::hardware::types::hardware_mode::HardwareMode;
use crate::hardware::types::timer_clock::TimerClock;

pub(crate) struct Timer {
    pub(crate) div: u8,
    pub(crate) tima: u8,
    pub(crate) tma: u8,
    pub(crate) tac: u8,
    sys_counter: u16,
    pub(crate) mode: HardwareMode,
    prev_bit: bool,
    overflow_pending: bool,
}

impl Timer {
    pub(crate) fn new() -> Self {
        Self {
            div: 0,
            tima: 0,
            tma: 0,
            tac: 0,
            sys_counter: 0,
            mode: HardwareMode::DMG,
            prev_bit: false,
            overflow_pending: false,
        }
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

    pub(crate) fn reset_div(&mut self) {
        let old_bit = self.timer_tick_bit();
        self.sys_counter = 0;
        self.div = 0;
        let new_bit = self.timer_tick_bit();
        if old_bit && !new_bit {
            self.increment_tima();
        }
        self.prev_bit = new_bit;
    }

    pub(crate) fn write_tima(&mut self, value: u8) {
        self.tima = value;
        self.overflow_pending = false;
    }

    pub(crate) fn write_tac(&mut self, value: u8) {
        let old_bit = self.timer_tick_bit();
        self.tac = value;
        let new_bit = self.timer_tick_bit();
        if old_bit && !new_bit {
            self.increment_tima();
        }
        self.prev_bit = new_bit;
    }

    fn increment_tima(&mut self) {
        let (new_tima, overflow) = self.tima.overflowing_add(1);
        if overflow {
            self.tima = self.tma;
            self.overflow_pending = true;
        } else {
            self.tima = new_tima;
        }
    }

    pub(crate) fn step(&mut self, cycles: u64) -> bool {
        let mut interrupt = false;

        for _ in 0..cycles {
            self.sys_counter = self.sys_counter.wrapping_add(1);
            self.div = (self.sys_counter >> 8) as u8;

            let new_bit = self.timer_tick_bit();
            if self.prev_bit && !new_bit {
                self.increment_tima();
            }
            self.prev_bit = new_bit;

            if self.overflow_pending {
                self.overflow_pending = false;
                interrupt = true;
            }
        }

        interrupt
    }
}

use crate::hardware::types::hardware_mode::HardwareMode;
use crate::hardware::types::timer_clock::TimerClock;
use crate::save_state::{StateReader, StateWriter, decode_hardware_mode};
use anyhow::Result;

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
            div: 0xAD,
            tima: 0,
            tma: 0,
            tac: 0,
            sys_counter: 0,
            mode: HardwareMode::DMG,
            prev_bit: false,
            overflow_pending: false,
        }
    }
    
    pub(crate) fn apply_bess_div(&mut self, div: u8) {
        self.div = div;
        self.sys_counter = (div as u16) << 8;
        self.prev_bit = false;
        self.overflow_pending = false;
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

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.div);
        writer.write_u8(self.tima);
        writer.write_u8(self.tma);
        writer.write_u8(self.tac);
        writer.write_u16(self.sys_counter);
        writer.write_u8(encode_hardware_mode(self.mode));
        writer.write_bool(self.prev_bit);
        writer.write_bool(self.overflow_pending);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            div: reader.read_u8()?,
            tima: reader.read_u8()?,
            tma: reader.read_u8()?,
            tac: reader.read_u8()?,
            sys_counter: reader.read_u16()?,
            mode: decode_hardware_mode(reader.read_u8()?)?,
            prev_bit: reader.read_bool()?,
            overflow_pending: reader.read_bool()?,
        })
    }
}

fn encode_hardware_mode(mode: HardwareMode) -> u8 {
    match mode {
        HardwareMode::DMG => 0,
        HardwareMode::SGB1 => 1,
        HardwareMode::SGB2 => 2,
        HardwareMode::CGBNormal => 3,
        HardwareMode::CGBDouble => 4,
    }
}

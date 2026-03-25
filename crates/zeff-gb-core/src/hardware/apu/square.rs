use super::Apu;
use crate::hardware::types::constants::{NR10, NR13, NR14, NR23, NR24};

const DUTY_TABLE: [[bool; 8]; 4] = [
    [false, false, false, false, false, false, false, true],
    [true, false, false, false, false, false, false, true],
    [true, false, false, false, false, true, true, true],
    [false, true, true, true, true, true, true, false],
];

impl Apu {
    pub(super) fn ch1_frequency(&self) -> u16 {
        let low = self.regs[(NR13 - NR10) as usize] as u16;
        let high = (self.regs[(NR14 - NR10) as usize] as u16) & 0x07;
        (high << 8) | low
    }

    pub(super) fn ch2_frequency(&self) -> u16 {
        let low = self.regs[(NR23 - NR10) as usize] as u16;
        let high = (self.regs[(NR24 - NR10) as usize] as u16) & 0x07;
        (high << 8) | low
    }

    pub(super) fn advance_square_channel(&mut self, channel_index: usize, t_cycles: u64) {
        if !self.channels[channel_index].enabled {
            return;
        }

        let period = if channel_index == 0 {
            self.square_period_t_cycles(self.ch1_frequency())
        } else {
            self.square_period_t_cycles(self.ch2_frequency())
        };

        let timer = if channel_index == 0 {
            &mut self.ch1_timer
        } else {
            &mut self.ch2_timer
        };
        if *timer == 0 {
            *timer = period;
        }

        let mut remaining = t_cycles;
        while remaining >= *timer {
            remaining -= *timer;
            *timer = period;
            if channel_index == 0 {
                self.ch1_duty_pos = (self.ch1_duty_pos + 1) & 0x07;
            } else {
                self.ch2_duty_pos = (self.ch2_duty_pos + 1) & 0x07;
            }
        }
        *timer -= remaining;
    }

    pub(super) fn square_sample(&self, channel_index: usize, duty_pos: u8, duty_reg: u8) -> f32 {
        if !self.channels[channel_index].enabled {
            return 0.0;
        }
        let duty = ((duty_reg >> 6) & 0x03) as usize;
        let high = DUTY_TABLE[duty][duty_pos as usize];
        let volume = self.channels[channel_index].envelope_volume as f32 / 15.0;
        if high { volume } else { -volume }
    }
}

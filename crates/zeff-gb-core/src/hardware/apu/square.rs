use super::Apu;
use crate::hardware::types::constants::{NR10, NR11, NR13, NR14, NR21, NR23, NR24};

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
            self.set_square_just_reloaded(channel_index, false);
            return;
        }

        let mut just_reloaded = false;
        let output_delay = if channel_index == 0 {
            &mut self.ch1_output_delay
        } else {
            &mut self.ch2_output_delay
        };
        let mut remaining = t_cycles;
        if *output_delay != 0 {
            if *output_delay > remaining {
                *output_delay -= remaining;
                return;
            }

            remaining -= *output_delay;
            *output_delay = 0;
            self.set_square_output_suppressed(channel_index, false);
            self.advance_square_duty_position(channel_index);
            if remaining == 0 {
                self.set_square_just_reloaded(channel_index, true);
                return;
            }
        }

        let period = if channel_index == 0 {
            self.square_period_t_cycles(self.ch1_frequency())
        } else {
            self.square_period_t_cycles(self.ch2_frequency())
        };

        let mut timer = if channel_index == 0 {
            self.ch1_timer
        } else {
            self.ch2_timer
        };
        if timer == 0 {
            timer = period;
        }

        while remaining >= timer {
            remaining -= timer;
            timer = period;
            self.advance_square_duty_position(channel_index);
            just_reloaded = remaining == 0;
        }
        if remaining != 0 {
            timer -= remaining;
            just_reloaded = false;
        }
        if channel_index == 0 {
            self.ch1_timer = timer;
        } else {
            self.ch2_timer = timer;
        }
        self.set_square_just_reloaded(channel_index, just_reloaded);
    }

    fn advance_square_duty_position(&mut self, channel_index: usize) {
        if channel_index == 0 {
            self.ch1_duty_pos = (self.ch1_duty_pos + 1) & 0x07;
            self.ch1_current_duty = (self.regs[(NR11 - NR10) as usize] >> 6) & 0x03;
        } else {
            self.ch2_duty_pos = (self.ch2_duty_pos + 1) & 0x07;
            self.ch2_current_duty = (self.regs[(NR21 - NR10) as usize] >> 6) & 0x03;
        }
    }

    pub(super) fn maybe_apply_square_duty_write(&mut self, addr: u16, value: u8) {
        match addr {
            NR11 if self.ch1_output_delay != 0 => {
                self.ch1_current_duty = (value >> 6) & 0x03;
            }
            NR21 if self.ch2_output_delay != 0 => {
                self.ch2_current_duty = (value >> 6) & 0x03;
            }
            _ => {}
        }
    }

    pub(super) fn maybe_apply_square_frequency_write(
        &mut self,
        addr: u16,
        value: u8,
        old_value: u8,
    ) {
        self.maybe_apply_double_speed_high_frequency_boundary_write(addr, value, old_value);

        match addr {
            NR13 | NR14
                if self.channels[0].enabled
                    && (self.ch1_just_reloaded || self.ch1_output_delay != 0) =>
            {
                self.ch1_timer = self.square_period_t_cycles(self.ch1_frequency());
            }
            NR23 | NR24
                if self.channels[1].enabled
                    && (self.ch2_just_reloaded || self.ch2_output_delay != 0) =>
            {
                self.ch2_timer = self.square_period_t_cycles(self.ch2_frequency());
            }
            _ => {}
        }
    }

    fn maybe_apply_double_speed_high_frequency_boundary_write(
        &mut self,
        addr: u16,
        value: u8,
        old_value: u8,
    ) {
        if !self.cgb_double_speed
            || value & 0x80 != 0
            || (old_value & 0x07) != 0x07
            || (value & 0x07) == 0x07
        {
            return;
        }

        match addr {
            NR14 if self.channels[0].enabled && self.ch1_output_delay == 0 => {
                let old_freq = ((u16::from(old_value) & 0x07) << 8)
                    | u16::from(self.regs[(NR13 - NR10) as usize]);
                if self.ch1_timer + 1 == self.square_period_t_cycles(old_freq) {
                    self.ch1_timer = self.square_period_t_cycles(self.ch1_frequency());
                }
            }
            NR24 if self.channels[1].enabled && self.ch2_output_delay == 0 => {
                let old_freq = ((u16::from(old_value) & 0x07) << 8)
                    | u16::from(self.regs[(NR23 - NR10) as usize]);
                if self.ch2_timer + 1 == self.square_period_t_cycles(old_freq) {
                    self.ch2_timer = self.square_period_t_cycles(self.ch2_frequency());
                }
            }
            _ => {}
        }
    }

    pub(super) fn square_sample(&self, channel_index: usize, duty_pos: u8) -> f32 {
        if !self.channels[channel_index].enabled || self.square_output_suppressed(channel_index) {
            return 0.0;
        }
        let duty = self.square_current_duty(channel_index) as usize;
        let high = DUTY_TABLE[duty][duty_pos as usize];
        let volume = self.channels[channel_index].envelope_volume as f32 / 15.0;
        if high { volume } else { -volume }
    }

    pub(super) fn square_pcm_output(&self, channel_index: usize, duty_pos: u8) -> u8 {
        if !self.channels[channel_index].enabled || self.square_output_suppressed(channel_index) {
            return 0;
        }
        let duty = self.square_current_duty(channel_index) as usize;
        if DUTY_TABLE[duty][duty_pos as usize] {
            self.channels[channel_index].envelope_volume & 0x0F
        } else {
            0
        }
    }

    fn square_output_suppressed(&self, channel_index: usize) -> bool {
        if channel_index == 0 {
            self.ch1_output_suppressed
        } else {
            self.ch2_output_suppressed
        }
    }

    fn set_square_output_suppressed(&mut self, channel_index: usize, suppressed: bool) {
        if channel_index == 0 {
            self.ch1_output_suppressed = suppressed;
        } else {
            self.ch2_output_suppressed = suppressed;
        }
    }

    fn set_square_just_reloaded(&mut self, channel_index: usize, just_reloaded: bool) {
        if channel_index == 0 {
            self.ch1_just_reloaded = just_reloaded;
        } else {
            self.ch2_just_reloaded = just_reloaded;
        }
    }

    fn square_current_duty(&self, channel_index: usize) -> u8 {
        if channel_index == 0 {
            self.ch1_current_duty
        } else {
            self.ch2_current_duty
        }
    }
}

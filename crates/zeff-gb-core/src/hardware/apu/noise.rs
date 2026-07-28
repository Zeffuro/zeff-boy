use super::Apu;
use crate::hardware::types::constants::{NR10, NR42, NR43};

impl Apu {
    pub(super) fn maybe_apply_noise_frequency_write(
        &mut self,
        addr: u16,
        value: u8,
        old_value: u8,
    ) {
        if addr != NR43 || value == old_value {
            return;
        }

        if !self.channels[3].enabled || self.ch4_counter_countdown == 0 {
            return;
        }

        let old_divisor = noise_counter_divisor_from_nr43(old_value);
        let new_divisor = noise_counter_divisor_from_nr43(value);

        if new_divisor < old_divisor {
            self.ch4_counter_countdown = self.ch4_counter_countdown.min(new_divisor);
        } else if new_divisor > old_divisor && self.ch4_counter_countdown == old_divisor {
            self.ch4_counter_countdown = new_divisor;
            if (old_value & 0x07) == 0 && (self.ch4_alignment & 0x03) == 0 {
                self.ch4_counter_countdown += 2;
            }
        }
    }

    pub(super) fn reset_noise_runtime(&mut self, was_active: bool) {
        self.ch4_counter_active = (self.regs[(NR42 - NR10) as usize] & 0xF8) != 0;
        let was_background_counting = self.ch4_background_counter_active;
        self.ch4_background_counter_active = true;

        let mut divisor_code = self.regs[(NR43 - NR10) as usize] & 0x07;
        let mut instant_lfsr_step = false;
        let mut divisor_one_glitch = false;

        if divisor_code > 1 && self.ch4_counter_countdown == 1 {
            self.ch4_counter = self.ch4_counter.wrapping_add(1) & 0x3FFF;
        } else if self.ch4_counter_countdown == 2 && (self.ch4_alignment & 0x03) == 0 && was_active
        {
            if divisor_code == 0 {
                divisor_code = 8;
            } else if divisor_code == 1 {
                if !self.ch4_did_step_counter {
                    divisor_one_glitch = true;
                }

                let mask = 1 << (self.regs[(NR43 - NR10) as usize] >> 4);
                let old_bit = (self.ch4_counter & mask) != 0;
                self.ch4_counter = self.ch4_counter.wrapping_add(1) & 0x3FFF;
                let new_bit = (self.ch4_counter & mask) != 0;
                instant_lfsr_step = new_bit && !old_bit;
            }
        }

        self.ch4_counter_countdown = if divisor_code == 0 {
            6
        } else {
            u64::from(divisor_code) * 4 + 6
        };

        match (self.ch4_alignment & 0x03, divisor_code) {
            (1 | 3, 0) => {
                if was_background_counting {
                    self.ch4_counter_countdown = self.ch4_counter_countdown.saturating_sub(1);
                } else {
                    self.ch4_counter_countdown += 1;
                }
            }
            (3, _) => {
                self.ch4_counter_countdown = self.ch4_counter_countdown.saturating_sub(3);
            }
            (1, _) => {
                self.ch4_counter_countdown = self.ch4_counter_countdown.saturating_sub(1);
                if divisor_code == 1
                    && was_active
                    && (self.regs[(NR43 - NR10) as usize] & 0xF0) == 0
                {
                    self.ch4_counter_countdown = self.ch4_counter_countdown.saturating_sub(4);
                }
            }
            (2, _) if divisor_code != 0 => {
                self.ch4_counter_countdown = self.ch4_counter_countdown.saturating_sub(2);
            }
            (0, _) if divisor_code > 1 => {
                self.ch4_counter_countdown = self.ch4_counter_countdown.saturating_sub(4);
            }
            (0, 1) if was_active && (self.regs[(NR43 - NR10) as usize] & 0xF0) == 0 => {
                self.ch4_counter_countdown = self.ch4_counter_countdown.saturating_sub(4);
            }
            _ => {}
        }

        if divisor_code > 1 && !self.ch4_counter_active && (self.ch4_alignment & 0x03) == 0 {
            self.ch4_counter_countdown += 4;
        } else if divisor_code <= 1
            && was_background_counting
            && !was_active
            && (self.ch4_alignment & 0x03) == 0
            && divisor_code != 0
        {
            self.ch4_counter_countdown = self.ch4_counter_countdown.saturating_sub(4);
        }

        if divisor_one_glitch {
            self.ch4_counter_countdown = self.ch4_counter_countdown.saturating_sub(4);
        }

        self.ch4_lfsr = 0x7FFF;
        self.ch4_did_step_counter = (self.ch4_alignment & 0x03) == 2;
        self.ch4_countdown_reloaded = false;

        if instant_lfsr_step {
            self.step_noise_lfsr();
        }
    }

    pub(super) fn reset_noise_divider_state(&mut self) {
        self.ch4_timer = 0;
        self.noise_cycle_accum = 0;
        self.ch4_lfsr = 0x7FFF;
        self.ch4_counter = 0;
        self.ch4_counter_countdown = 0;
        self.ch4_alignment = 0;
        self.ch4_counter_active = false;
        self.ch4_background_counter_active = false;
        self.ch4_did_step_counter = false;
        self.ch4_countdown_reloaded = false;
    }

    pub(super) fn advance_noise_clock(&mut self, t_cycles: u64) {
        self.noise_cycle_accum = self.noise_cycle_accum.wrapping_add(t_cycles);
        while self.noise_cycle_accum >= 2 {
            self.noise_cycle_accum -= 2;
            self.ch4_alignment = self.ch4_alignment.wrapping_add(1);
            self.advance_noise_channel_2mhz(1);
        }
    }

    fn advance_noise_channel_2mhz(&mut self, cycles: u64) {
        if !self.ch4_counter_active && !self.ch4_background_counter_active {
            return;
        }

        let divisor = self.noise_counter_divisor_2mhz();
        if self.ch4_counter_countdown == 0 {
            self.ch4_counter_countdown = divisor;
        }

        let mut remaining = cycles;
        while remaining >= self.ch4_counter_countdown {
            remaining -= self.ch4_counter_countdown;
            self.ch4_counter_countdown = divisor;

            let mask = 1 << (self.regs[(NR43 - NR10) as usize] >> 4);
            let old_bit = (self.ch4_counter & mask) != 0;
            self.ch4_counter = self.ch4_counter.wrapping_add(1) & 0x3FFF;
            self.ch4_did_step_counter = true;
            let new_bit = (self.ch4_counter & mask) != 0;

            if self.channels[3].enabled && new_bit && !old_bit {
                self.step_noise_lfsr();
            }
        }

        if remaining > 0 {
            self.ch4_counter_countdown = self.ch4_counter_countdown.saturating_sub(remaining);
            self.ch4_countdown_reloaded = false;
        } else {
            self.ch4_countdown_reloaded = true;
        }
    }

    fn noise_counter_divisor_2mhz(&self) -> u64 {
        noise_counter_divisor_from_nr43(self.regs[(NR43 - NR10) as usize])
    }

    fn step_noise_lfsr(&mut self) {
        let xor = (self.ch4_lfsr & 0x01) ^ ((self.ch4_lfsr >> 1) & 0x01);
        self.ch4_lfsr = (self.ch4_lfsr >> 1) | (xor << 14);
        if (self.regs[(NR43 - NR10) as usize] & 0x08) != 0 {
            self.ch4_lfsr = (self.ch4_lfsr & !(1 << 6)) | (xor << 6);
        }
    }

    pub(super) fn ch4_sample(&self) -> f32 {
        if !self.channels[3].enabled {
            return 0.0;
        }
        let volume = self.channels[3].envelope_volume as f32 / 15.0;
        if (self.ch4_lfsr & 0x01) == 0 {
            volume
        } else {
            -volume
        }
    }

    pub(super) fn ch4_pcm_output(&self) -> u8 {
        if !self.channels[3].enabled {
            return 0;
        }
        if (self.ch4_lfsr & 0x01) == 0 {
            self.channels[3].envelope_volume & 0x0F
        } else {
            0
        }
    }
}

fn noise_counter_divisor_from_nr43(value: u8) -> u64 {
    let divisor = u64::from((value & 0x07) << 2);
    if divisor == 0 { 2 } else { divisor }
}

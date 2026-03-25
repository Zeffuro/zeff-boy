use super::Apu;
use crate::hardware::types::constants::{NR10, NR43};

impl Apu {
    pub(super) fn noise_period_t_cycles(&self) -> u64 {
        let nr43 = self.regs[(NR43 - NR10) as usize];
        let shift = (nr43 >> 4) & 0x0F;
        let divisor_code = nr43 & 0x07;
        let divisor = match divisor_code {
            0 => 8u32,
            1 => 16,
            2 => 32,
            3 => 48,
            4 => 64,
            5 => 80,
            6 => 96,
            _ => 112,
        };
        u64::from(divisor << shift).max(8)
    }

    pub(super) fn advance_noise_channel(&mut self, t_cycles: u64) {
        if !self.channels[3].enabled {
            return;
        }

        let period = self.noise_period_t_cycles();
        if self.ch4_timer == 0 {
            self.ch4_timer = period;
        }

        let mut remaining = t_cycles;
        while remaining >= self.ch4_timer {
            remaining -= self.ch4_timer;
            self.ch4_timer = period;

            let xor = (self.ch4_lfsr & 0x01) ^ ((self.ch4_lfsr >> 1) & 0x01);
            self.ch4_lfsr = (self.ch4_lfsr >> 1) | (xor << 14);
            if (self.regs[(NR43 - NR10) as usize] & 0x08) != 0 {
                self.ch4_lfsr = (self.ch4_lfsr & !(1 << 6)) | (xor << 6);
            }
        }
        self.ch4_timer -= remaining;
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
}

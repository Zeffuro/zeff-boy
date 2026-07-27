use super::*;

impl Psg {
    pub(super) fn ch3_frequency(&self) -> u16 {
        let low = self.regs[(NR33 - NR10) as usize] as u16;
        let high = (self.regs[(NR34 - NR10) as usize] as u16) & 0x07;
        (high << 8) | low
    }

    pub(super) fn wave_period_t_cycles(&self, freq: u16) -> u64 {
        let base = 2048u16.saturating_sub(freq.max(1));
        u64::from(base.max(1)) * 2
    }

    pub(super) fn maybe_apply_wave_frequency_write(&mut self, addr: u16) {
        if !matches!(addr, NR33 | NR34) || !self.channels[2].enabled || self.ch3_output_delay == 0 {
            return;
        }

        self.ch3_timer = self.wave_period_t_cycles(self.ch3_frequency());
    }

    pub(super) fn advance_wave_channel(&mut self, t_cycles: u64) {
        if !self.channels[2].enabled {
            return;
        }

        let mut remaining = t_cycles;
        if self.ch3_output_delay != 0 {
            if self.ch3_output_delay > remaining {
                self.ch3_output_delay -= remaining;
                return;
            }

            remaining -= self.ch3_output_delay;
            self.ch3_output_delay = 0;
            if self.ch3_restart_pending {
                self.ch3_restart_pending = false;
                self.ch3_wave_pos = 1;
            }
            if remaining == 0 {
                return;
            }
        }

        let period = self.wave_period_t_cycles(self.ch3_frequency());
        if self.ch3_timer == 0 {
            self.ch3_timer = period;
        }

        while remaining >= self.ch3_timer {
            remaining -= self.ch3_timer;
            self.ch3_timer = period;
            self.ch3_wave_pos = (self.ch3_wave_pos + 1) & 0x1F;
        }
        self.ch3_timer -= remaining;
    }

    pub(super) fn ch3_sample(&self) -> f32 {
        if !self.channels[2].enabled
            || (self.ch3_output_delay != 0 && !self.ch3_restart_pending)
            || (self.regs[(NR30 - NR10) as usize] & 0x80) == 0
        {
            return 0.0;
        }

        let wave_byte = self.wave_ram[(self.ch3_wave_pos / 2) as usize];
        let raw = if (self.ch3_wave_pos & 1) == 0 {
            wave_byte >> 4
        } else {
            wave_byte & 0x0F
        };

        let scaled = match (self.regs[(NR32 - NR10) as usize] >> 5) & 0x03 {
            0 => 0,
            1 => raw,
            2 => raw >> 1,
            _ => raw >> 2,
        };

        (scaled as f32 / 15.0) * 2.0 - 1.0
    }
}

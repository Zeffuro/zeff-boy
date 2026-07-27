use super::Apu;
use crate::hardware::types::constants::{NR10, NR30, NR32, NR33, NR34};

impl Apu {
    pub(super) fn read_wave_ram_cpu(&self, addr: u16) -> u8 {
        match self.wave_ram_cpu_access_index(addr) {
            Some(index) => self.wave_ram[index],
            None => 0xFF,
        }
    }

    pub(super) fn write_wave_ram_cpu(&mut self, addr: u16, value: u8) {
        if let Some(index) = self.wave_ram_cpu_access_index(addr) {
            self.wave_ram[index] = value;
        }
    }

    fn wave_ram_cpu_access_index(&self, addr: u16) -> Option<usize> {
        if !self.channels[2].enabled {
            return Some((addr - crate::hardware::types::constants::WAVE_RAM_START) as usize);
        }

        if !self.cgb_hardware && self.ch3_wave_access_window == 0 {
            return None;
        }
        if self.ch3_restart_pending && self.ch3_output_delay != 0 {
            return Some(0);
        }
        if !self.cgb_hardware {
            return Some(self.ch3_wave_access_index);
        }

        Some((self.ch3_wave_pos / 2) as usize)
    }

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

    pub(super) fn maybe_apply_dmg_wave_retrigger_corruption(&mut self, was_active: bool) {
        if self.cgb_hardware || !was_active {
            return;
        }

        let Some(index) = self.dmg_wave_retrigger_corruption_index() else {
            return;
        };
        let index = index & 0x0F;
        if index < 4 {
            self.wave_ram[0] = self.wave_ram[index];
        } else {
            let block_start = index & !0x03;
            let block = [
                self.wave_ram[block_start],
                self.wave_ram[block_start + 1],
                self.wave_ram[block_start + 2],
                self.wave_ram[block_start + 3],
            ];
            self.wave_ram[..4].copy_from_slice(&block);
        }
    }

    fn dmg_wave_retrigger_corruption_index(&self) -> Option<usize> {
        if self.ch3_output_delay != 0 {
            return None;
        }

        (self.ch3_timer == 2).then_some((((self.ch3_wave_pos + 1) & 0x1F) / 2) as usize)
    }

    pub(super) fn advance_wave_channel(&mut self, t_cycles: u64) {
        self.ch3_wave_access_window = self.ch3_wave_access_window.saturating_sub(t_cycles);

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
            self.open_ch3_wave_access_window();
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
            self.open_ch3_wave_access_window();
        }
        self.ch3_timer -= remaining;
    }

    fn open_ch3_wave_access_window(&mut self) {
        self.ch3_wave_access_index = (self.ch3_wave_pos / 2) as usize;
        self.ch3_wave_access_window = 2;
    }

    pub(super) fn ch3_sample(&self) -> f32 {
        if !self.channels[2].enabled
            || (self.ch3_output_delay != 0 && !self.ch3_restart_pending)
            || (self.regs[(NR30 - NR10) as usize] & 0x80) == 0
        {
            return 0.0;
        }

        let scaled = self.ch3_pcm_output();
        (scaled as f32 / 15.0) * 2.0 - 1.0
    }

    pub(super) fn ch3_pcm_output(&self) -> u8 {
        if !self.channels[2].enabled
            || (self.ch3_output_delay != 0 && !self.ch3_restart_pending)
            || (self.regs[(NR30 - NR10) as usize] & 0x80) == 0
        {
            return 0;
        }

        let wave_byte = self.wave_ram[(self.ch3_wave_pos / 2) as usize];
        let raw = if (self.ch3_wave_pos & 1) == 0 {
            wave_byte >> 4
        } else {
            wave_byte & 0x0F
        };

        match (self.regs[(NR32 - NR10) as usize] >> 5) & 0x03 {
            0 => 0,
            1 => raw,
            2 => raw >> 1,
            _ => raw >> 2,
        }
    }
}

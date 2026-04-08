use super::Apu;
use crate::hardware::types::constants::*;

impl Apu {
    pub(super) fn frame_sequencer_step(&mut self) {
        let step = self.frame_seq_step;
        if matches!(step, 0 | 2 | 4 | 6) {
            self.clock_length();
        }
        if matches!(step, 2 | 6) {
            self.clock_sweep();
        }
        if step == 7 {
            self.clock_envelope();
        }
    }

    fn clock_length(&mut self) {
        for channel in &mut self.channels {
            if channel.length_enabled && channel.length_counter > 0 {
                channel.length_counter -= 1;
                if channel.length_counter == 0 {
                    channel.enabled = false;
                }
            }
        }
        self.update_nr52_status();
    }

    fn clock_sweep(&mut self) {
        let (current_shadow, shift, negate) = {
            let ch1 = &mut self.channels[0];
            if !ch1.enabled || !ch1.sweep_enabled {
                return;
            }

            if ch1.sweep_timer > 0 {
                ch1.sweep_timer -= 1;
            }
            if ch1.sweep_timer != 0 {
                return;
            }

            ch1.sweep_timer = sweep_period_or_8(ch1.sweep_period);

            (ch1.sweep_shadow_freq, ch1.sweep_shift, ch1.sweep_negate)
        };

        let Some(new_freq) = sweep_calculation(current_shadow, shift, negate) else {
            let ch1 = &mut self.channels[0];
            ch1.enabled = false;
            ch1.sweep_enabled = false;
            self.update_nr52_status();
            return;
        };

        if shift > 0 {
            self.set_ch1_frequency(new_freq);
            let overflow = sweep_calculation(new_freq, shift, negate).is_none();
            let ch1 = &mut self.channels[0];
            if negate {
                ch1.sweep_negate_used = true;
            }
            ch1.sweep_shadow_freq = new_freq;
            if overflow {
                ch1.enabled = false;
                ch1.sweep_enabled = false;
                self.update_nr52_status();
            }
        }
    }

    fn clock_envelope(&mut self) {
        for &channel_index in &[0usize, 1, 3] {
            let channel = &mut self.channels[channel_index];
            if !channel.enabled || channel.envelope_period == 0 {
                continue;
            }

            if channel.envelope_timer > 0 {
                channel.envelope_timer -= 1;
            }

            if channel.envelope_timer == 0 {
                channel.envelope_timer = envelope_period_or_8(channel.envelope_period);
                if channel.envelope_increase {
                    if channel.envelope_volume < 0x0F {
                        channel.envelope_volume += 1;
                    }
                } else if channel.envelope_volume > 0 {
                    channel.envelope_volume -= 1;
                }
            }
        }
    }

    pub(super) fn update_nr52_status(&mut self) {
        let mut active_bits = 0u8;
        for (i, channel) in self.channels.iter().enumerate() {
            if channel.enabled {
                active_bits |= 1 << i;
            }
        }
        self.nr52 = (self.nr52 & 0x80) | active_bits;
    }

    pub(super) fn channel_dac_enabled(&self, channel_index: usize) -> bool {
        match channel_index {
            0 => (self.regs[(NR12 - NR10) as usize] & 0xF8) != 0,
            1 => (self.regs[(NR22 - NR10) as usize] & 0xF8) != 0,
            2 => (self.regs[(NR30 - NR10) as usize] & 0x80) != 0,
            3 => (self.regs[(NR42 - NR10) as usize] & 0xF8) != 0,
            _ => false,
        }
    }

    pub(super) fn maybe_apply_dac_gate(&mut self, addr: u16) {
        let channel_index = match addr {
            NR12 => Some(0usize),
            NR22 => Some(1usize),
            NR30 => Some(2usize),
            NR42 => Some(3usize),
            _ => None,
        };

        if let Some(channel_index) = channel_index
            && !self.channel_dac_enabled(channel_index)
        {
            self.channels[channel_index].enabled = false;
            if channel_index == 0 {
                self.channels[0].sweep_enabled = false;
            }
            self.update_nr52_status();
        }
    }

    pub(super) fn maybe_write_length(&mut self, addr: u16, value: u8) {
        if let Some(channel_index) = length_channel_from_addr(addr) {
            let max_length = channel_max_length(channel_index);
            let length_data = match addr {
                NR31 => value as u16,
                _ => (value & 0x3F) as u16,
            };
            self.channels[channel_index].length_counter = max_length.saturating_sub(length_data);
        }
    }

    pub(super) fn maybe_write_length_enable(&mut self, addr: u16, value: u8) -> bool {
        if let Some(channel_index) = trigger_channel(addr).map(|(idx, _)| idx) {
            let was_enabled = self.channels[channel_index].length_enabled;
            let now_enabled = (value & 0x40) != 0;
            self.channels[channel_index].length_enabled = now_enabled;

            if !was_enabled && now_enabled && self.frame_seq_step_is_odd() {
                let clocked = self.clock_length_channel(channel_index);
                self.update_nr52_status();
                return clocked;
            }
        }

        false
    }

    fn clock_length_channel(&mut self, channel_index: usize) -> bool {
        let channel = &mut self.channels[channel_index];
        if channel.length_enabled && channel.length_counter > 0 {
            channel.length_counter -= 1;
            if channel.length_counter == 0 {
                channel.enabled = false;
            }
            return true;
        }

        false
    }

    pub(super) fn frame_seq_step_is_odd(&self) -> bool {
        (self.frame_seq_step & 0x01) != 0
    }

    pub(super) fn maybe_write_sweep(&mut self, addr: u16, value: u8) {
        if addr != NR10 {
            return;
        }

        let new_negate = (value & 0x08) != 0;
        let mut disable_channel = false;
        {
            let ch1 = &mut self.channels[0];
            if ch1.sweep_negate && !new_negate && ch1.sweep_negate_used {
                ch1.enabled = false;
                ch1.sweep_enabled = false;
                disable_channel = true;
            }

            ch1.sweep_period = (value >> 4) & 0x07;
            ch1.sweep_negate = new_negate;
            ch1.sweep_shift = value & 0x07;
        }

        if disable_channel {
            self.update_nr52_status();
        }
    }

    pub(super) fn maybe_write_envelope(&mut self, addr: u16, value: u8) {
        if let Some(channel_index) = envelope_channel_from_addr(addr) {
            self.channels[channel_index].envelope_period = value & 0x07;
            self.channels[channel_index].envelope_increase = (value & 0x08) != 0;
            self.channels[channel_index].envelope_volume = envelope_initial_volume(value);
        }
    }

    pub(super) fn init_sweep_on_trigger(&mut self) {
        let current_freq = self.ch1_frequency();
        let ch1 = &mut self.channels[0];
        ch1.sweep_shadow_freq = current_freq;
        ch1.sweep_timer = sweep_period_or_8(ch1.sweep_period);
        ch1.sweep_enabled = ch1.sweep_period != 0 || ch1.sweep_shift != 0;
        ch1.sweep_negate_used = false;
        if ch1.sweep_shift > 0
            && sweep_calculation(current_freq, ch1.sweep_shift, ch1.sweep_negate).is_none()
        {
            ch1.enabled = false;
            ch1.sweep_enabled = false;
        }
    }

    pub(super) fn set_ch1_frequency(&mut self, freq: u16) {
        let idx13 = (NR13 - NR10) as usize;
        let idx14 = (NR14 - NR10) as usize;
        self.regs[idx13] = (freq & 0xFF) as u8;
        self.regs[idx14] = (self.regs[idx14] & !0x07) | ((freq >> 8) as u8 & 0x07);
    }

    pub(super) fn reset_channel_runtime(&mut self, channel_index: usize) {
        match channel_index {
            0 => {
                self.ch1_duty_pos = 0;
                self.ch1_timer = self.square_period_t_cycles(self.ch1_frequency());
            }
            1 => {
                self.ch2_duty_pos = 0;
                self.ch2_timer = self.square_period_t_cycles(self.ch2_frequency());
            }
            2 => {
                self.ch3_wave_pos = 0;
                self.ch3_timer = self.wave_period_t_cycles(self.ch3_frequency());
            }
            3 => {
                self.ch4_lfsr = 0x7FFF;
                self.ch4_timer = self.noise_period_t_cycles();
            }
            _ => {}
        }
    }
}

pub(super) fn trigger_channel(addr: u16) -> Option<(usize, u8)> {
    match addr {
        NR14 => Some((0, 0x01)),
        NR24 => Some((1, 0x02)),
        NR34 => Some((2, 0x04)),
        NR44 => Some((3, 0x08)),
        _ => None,
    }
}

fn length_channel_from_addr(addr: u16) -> Option<usize> {
    match addr {
        NR11 => Some(0),
        NR21 => Some(1),
        NR31 => Some(2),
        NR41 => Some(3),
        _ => None,
    }
}

pub(super) fn channel_max_length(channel_index: usize) -> u16 {
    match channel_index {
        0 | 1 | 3 => 64,
        2 => 256,
        _ => 64,
    }
}

pub(super) fn uses_envelope(channel_index: usize) -> bool {
    matches!(channel_index, 0 | 1 | 3)
}

fn envelope_channel_from_addr(addr: u16) -> Option<usize> {
    match addr {
        NR12 => Some(0),
        NR22 => Some(1),
        NR42 => Some(3),
        _ => None,
    }
}

pub(super) fn envelope_reg_index(channel_index: usize) -> usize {
    match channel_index {
        0 => (NR12 - NR10) as usize,
        1 => (NR22 - NR10) as usize,
        3 => (NR42 - NR10) as usize,
        _ => 0,
    }
}

pub(super) fn envelope_initial_volume(reg: u8) -> u8 {
    (reg >> 4) & 0x0F
}

pub(super) fn envelope_period_or_8(period: u8) -> u8 {
    if period == 0 { 8 } else { period }
}

fn sweep_period_or_8(period: u8) -> u8 {
    if period == 0 { 8 } else { period }
}

fn sweep_calculation(shadow_freq: u16, shift: u8, negate: bool) -> Option<u16> {
    if shift == 0 {
        return Some(shadow_freq);
    }
    let delta = shadow_freq >> shift;
    if negate {
        shadow_freq.checked_sub(delta)
    } else {
        let next = shadow_freq.saturating_add(delta);
        if next > 2047 { None } else { Some(next) }
    }
}

use super::Apu;
use crate::hardware::types::constants::*;

impl Apu {
    const CH1_SWEEP_TRIGGER_VISIBILITY_DELAY_T_CYCLES: u64 = 8;

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
        if self.ch1_sweep_trigger_visibility_delay != 0 {
            return;
        }

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

            if ch1.sweep_period == 0 {
                return;
            }

            (ch1.sweep_shadow_freq, ch1.sweep_shift, ch1.sweep_negate)
        };

        let Some(new_freq) = sweep_calculation(current_shadow, shift, negate) else {
            self.schedule_ch1_sweep_disable(shift);
            return;
        };

        if negate {
            self.channels[0].sweep_negate_used = true;
        }

        if shift > 0 {
            self.set_ch1_frequency(new_freq);
            let overflow = sweep_calculation(new_freq, shift, negate).is_none();
            let ch1 = &mut self.channels[0];
            ch1.sweep_shadow_freq = new_freq;
            if overflow {
                self.schedule_ch1_sweep_disable(shift);
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
                let armed_from_zero_period = channel.envelope_zero_period_arm;
                channel.envelope_zero_period_arm = false;
                channel.envelope_timer = if armed_from_zero_period {
                    channel.envelope_period.saturating_sub(1)
                } else {
                    envelope_period_or_8(channel.envelope_period)
                };
                self.tick_envelope_volume(channel_index);
            }
        }
    }

    pub(super) fn clock_forced_envelope_ticks(&mut self) {
        for channel_index in [0usize, 1, 3] {
            let channel = &mut self.channels[channel_index];
            if channel.envelope_forced_tick_delay == 0 {
                continue;
            }

            channel.envelope_forced_tick_delay -= 1;
            if channel.envelope_forced_tick_delay == 0 {
                self.tick_envelope_volume(channel_index);
            }
        }
    }

    fn tick_envelope_volume(&mut self, channel_index: usize) {
        let channel = &mut self.channels[channel_index];
        if !channel.enabled || channel.envelope_period == 0 {
            return;
        }

        if channel.envelope_increase {
            if channel.envelope_volume < 0x0F {
                channel.envelope_volume += 1;
            }
        } else if channel.envelope_volume > 0 {
            channel.envelope_volume -= 1;
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

        let new_period = (value >> 4) & 0x07;
        let new_negate = (value & 0x08) != 0;
        let new_shift = value & 0x07;
        let mut disable_channel = false;
        {
            let ch1 = &mut self.channels[0];
            if ch1.sweep_negate && !new_negate && ch1.sweep_negate_used {
                ch1.enabled = false;
                ch1.sweep_enabled = false;
                disable_channel = true;
            }

            ch1.sweep_period = new_period;
            ch1.sweep_negate = new_negate;
            ch1.sweep_shift = new_shift;
        }

        if new_shift == 0 || new_negate {
            self.ch1_sweep_pending_disable_delay = 0;
        }

        if disable_channel {
            self.update_nr52_status();
        }
    }

    pub(super) fn maybe_write_envelope(&mut self, addr: u16, value: u8, old_value: u8) {
        if let Some(channel_index) = envelope_channel_from_addr(addr) {
            let old_period = old_value & 0x07;
            let new_period = value & 0x07;
            self.channels[channel_index].envelope_zero_period_arm = false;
            if self.channels[channel_index].enabled && (value & 0xF8) != 0 {
                apply_nrx2_write_glitch(
                    &mut self.channels[channel_index].envelope_volume,
                    value,
                    old_value,
                );
                if old_period == 0
                    && new_period != 0
                    && self.channels[channel_index].envelope_timer == 0
                {
                    self.channels[channel_index].envelope_timer = 1;
                    self.channels[channel_index].envelope_zero_period_arm = true;
                    if new_period == 1 {
                        self.channels[channel_index].envelope_forced_tick_delay =
                            if self.div_apu_phase_high { 2 } else { 1 };
                    }
                }
            }
            self.channels[channel_index].envelope_period = new_period;
            self.channels[channel_index].envelope_increase = (value & 0x08) != 0;
        }
    }

    pub(super) fn init_sweep_on_trigger(&mut self) {
        self.ch1_sweep_pending_disable_delay = 0;
        self.ch1_sweep_trigger_visibility_delay = Self::CH1_SWEEP_TRIGGER_VISIBILITY_DELAY_T_CYCLES;
        let current_freq = self.ch1_frequency();
        let ch1 = &mut self.channels[0];
        ch1.sweep_shadow_freq = current_freq;
        ch1.sweep_timer = sweep_period_or_8(ch1.sweep_period);
        ch1.sweep_enabled = ch1.sweep_period != 0 || ch1.sweep_shift != 0;
        ch1.sweep_negate_used = false;
        if ch1.sweep_shift > 0 && {
            if ch1.sweep_negate {
                ch1.sweep_negate_used = true;
            }
            sweep_calculation(current_freq, ch1.sweep_shift, ch1.sweep_negate).is_none()
        } {
            let shift = ch1.sweep_shift;
            self.schedule_ch1_sweep_disable(shift);
        }
    }

    pub(super) fn clock_sweep_trigger_visibility_delay(&mut self, t_cycles: u64) {
        self.ch1_sweep_trigger_visibility_delay = self
            .ch1_sweep_trigger_visibility_delay
            .saturating_sub(t_cycles);
    }

    pub(super) fn clock_delayed_sweep_disable(&mut self, t_cycles: u64) {
        if self.ch1_sweep_pending_disable_delay == 0 {
            return;
        }

        if self.ch1_sweep_pending_disable_delay > t_cycles {
            self.ch1_sweep_pending_disable_delay -= t_cycles;
            return;
        }

        self.ch1_sweep_pending_disable_delay = 0;
        self.channels[0].enabled = false;
        self.channels[0].sweep_enabled = false;
        self.update_nr52_status();
    }

    fn schedule_ch1_sweep_disable(&mut self, shift: u8) {
        self.ch1_sweep_pending_disable_delay = self.sweep_calculation_delay_t_cycles(shift);
    }

    fn sweep_calculation_delay_t_cycles(&self, shift: u8) -> u64 {
        (u64::from(shift) + 2) * 4
    }

    pub(super) fn set_ch1_frequency(&mut self, freq: u16) {
        let idx13 = (NR13 - NR10) as usize;
        let idx14 = (NR14 - NR10) as usize;
        self.regs[idx13] = (freq & 0xFF) as u8;
        self.regs[idx14] = (self.regs[idx14] & !0x07) | ((freq >> 8) as u8 & 0x07);
    }

    pub(super) fn reset_channel_runtime(&mut self, channel_index: usize, was_active: bool) {
        match channel_index {
            0 => {
                self.ch1_current_duty = (self.regs[(NR11 - NR10) as usize] >> 6) & 0x03;
                self.ch1_timer = self.square_period_t_cycles(self.ch1_frequency());
                self.ch1_output_delay = self.ch1_timer
                    + self.square_trigger_start_delay(was_active)
                    + self.square_trigger_phase_delay();
                self.ch1_just_reloaded = false;
                if !was_active {
                    self.ch1_output_suppressed = true;
                }
            }
            1 => {
                self.ch2_current_duty = (self.regs[(NR21 - NR10) as usize] >> 6) & 0x03;
                self.ch2_timer = self.square_period_t_cycles(self.ch2_frequency());
                self.ch2_output_delay = self.ch2_timer
                    + self.square_trigger_start_delay(was_active)
                    + self.square_trigger_phase_delay();
                self.ch2_just_reloaded = false;
                if !was_active {
                    self.ch2_output_suppressed = true;
                }
            }
            2 => {
                self.maybe_apply_dmg_wave_retrigger_corruption(was_active);
                self.ch3_wave_access_window = 0;
                self.ch3_wave_access_index = 0;
                self.ch3_restart_pending = was_active && self.ch3_output_delay == 0;
                if !self.ch3_restart_pending {
                    self.ch3_wave_pos = 1;
                }
                self.ch3_timer = self.wave_period_t_cycles(self.ch3_frequency());
                self.ch3_output_delay = self.ch3_timer + 6;
            }
            3 => {
                self.reset_noise_runtime(was_active);
            }
            _ => {}
        }
    }

    pub(super) fn square_trigger_start_delay(&self, was_active: bool) -> u64 {
        if was_active || (self.cgb_double_speed && (self.pulse_noise_cycle_accum & 0x03) == 1) {
            4
        } else {
            8
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

fn apply_nrx2_write_glitch(volume: &mut u8, value: u8, old_value: u8) {
    let mut should_tick = (value & 0x07) != 0 && (old_value & 0x07) == 0;
    let should_invert = ((value ^ old_value) & 0x08) != 0;

    if (value & 0x0F) == 0x08 && (old_value & 0x0F) == 0x08 {
        should_tick = true;
    }

    if should_invert {
        if (value & 0x08) != 0 {
            if (old_value & 0x07) == 0 {
                *volume ^= 0x0F;
            } else {
                *volume = 0x0E_u8.wrapping_sub(*volume) & 0x0F;
            }
            should_tick = false;
        } else {
            *volume = 0x10_u8.wrapping_sub(*volume) & 0x0F;
        }
    }

    if should_tick {
        if (value & 0x08) != 0 {
            *volume = volume.wrapping_add(1) & 0x0F;
        } else {
            *volume = volume.wrapping_sub(1) & 0x0F;
        }
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
    let delta = shadow_freq >> shift;
    if negate {
        shadow_freq.checked_sub(delta)
    } else {
        let next = shadow_freq.saturating_add(delta);
        if next > 2047 { None } else { Some(next) }
    }
}

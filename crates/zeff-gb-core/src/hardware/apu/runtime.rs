use super::frame_seq::{
    channel_max_length, envelope_initial_volume, envelope_reg_index, trigger_channel, uses_envelope,
};
use super::{Apu, DIV_APU_SKIP_INACTIVE, DIV_APU_SKIP_NEXT, DIV_APU_SKIP_REPLAY_FIRST_CLOCK};
use crate::hardware::types::constants::*;

impl Apu {
    #[inline]
    pub fn step(&mut self, t_cycles: u64) {
        if !self.apu_enabled {
            return;
        }

        let powered = self.powered();
        self.advance_channel_timer_clocks(t_cycles, powered);
        self.clock_delayed_sweep_disable(t_cycles);
        self.clock_sweep_trigger_visibility_delay(t_cycles);
        if !powered {
            return;
        }

        if self.debug_capture_enabled {
            self.capture_debug_samples(t_cycles);
        }
        if self.sample_generation_enabled {
            self.generate_samples(t_cycles);
        }

        self.update_nr52_status();
    }

    pub(in crate::hardware) fn clock_div_apu(&mut self) {
        if !self.apu_enabled || !self.powered() {
            return;
        }
        self.div_apu_phase_high = false;

        match self.div_apu_skip_state {
            DIV_APU_SKIP_NEXT => {
                self.div_apu_skip_state = DIV_APU_SKIP_REPLAY_FIRST_CLOCK;
                return;
            }
            DIV_APU_SKIP_REPLAY_FIRST_CLOCK => {
                self.div_apu_skip_state = DIV_APU_SKIP_INACTIVE;
                let next_step = self.frame_seq_step;
                self.frame_seq_step = 0;
                self.frame_sequencer_step();
                self.frame_seq_step = next_step;
                self.clock_forced_envelope_ticks();
                self.update_nr52_status();
                return;
            }
            _ => {}
        }

        self.frame_sequencer_step();
        self.clock_forced_envelope_ticks();
        self.frame_seq_step = (self.frame_seq_step + 1) & 0x07;
        self.update_nr52_status();
    }

    pub(in crate::hardware) fn clock_div_apu_secondary_event(&mut self) {
        if !self.apu_enabled || !self.powered() {
            return;
        }
        self.div_apu_phase_high = true;
    }

    pub(in crate::hardware) fn skip_next_div_apu_event_if(&mut self, condition: bool) {
        if condition {
            self.div_apu_skip_state = DIV_APU_SKIP_NEXT;
            self.frame_seq_step = 1;
        } else {
            self.div_apu_skip_state = DIV_APU_SKIP_INACTIVE;
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(8_000);
    }

    pub fn set_cgb_hardware(&mut self, enabled: bool) {
        self.cgb_hardware = enabled;
    }

    pub fn set_cgb_double_speed(&mut self, enabled: bool) {
        self.cgb_double_speed = enabled;
    }

    pub fn drain_samples(&mut self) -> Vec<f32> {
        let mut drained = Vec::with_capacity(self.sample_buffer.capacity());
        std::mem::swap(&mut drained, &mut self.sample_buffer);
        drained
    }

    pub fn drain_samples_into(&mut self, target: &mut Vec<f32>) {
        target.clear();
        target.extend_from_slice(&self.sample_buffer);
        self.sample_buffer.clear();
    }

    pub fn channel_snapshot(&self) -> super::ApuChannelSnapshot {
        use crate::hardware::types::constants::{NR10, NR32};

        super::ApuChannelSnapshot {
            ch1_enabled: self.channels[0].enabled,
            ch1_frequency: self.ch1_frequency(),
            ch1_volume: self.channels[0].envelope_volume,
            ch2_enabled: self.channels[1].enabled,
            ch2_frequency: self.ch2_frequency(),
            ch2_volume: self.channels[1].envelope_volume,
            ch3_enabled: self.channels[2].enabled,
            ch3_frequency: self.ch3_frequency(),
            ch3_output_level: (self.regs[(NR32 - NR10) as usize] >> 5) & 0x03,
            ch4_enabled: self.channels[3].enabled,
            ch4_volume: self.channels[3].envelope_volume,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            NR10..=NR52 => {
                if addr == NR52 {
                    return NR52_READ_MASK | (self.nr52 & 0x8F);
                }

                let val = self.regs[(addr - NR10) as usize];
                val | read_mask(addr)
            }
            WAVE_RAM_START..=WAVE_RAM_END => self.read_wave_ram_cpu(addr),
            CGB_PCM12 => self.pcm12(),
            CGB_PCM34 => self.pcm34(),
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        if addr == NR52 {
            if value & 0x80 == 0 {
                let length_counters = (!self.cgb_hardware)
                    .then_some(self.channels.map(|channel| channel.length_counter));
                self.nr52 = 0;
                self.regs.fill(0);
                self.frame_seq_cycle_accum = 0;
                self.frame_seq_step = 0;
                self.div_apu_phase_high = false;
                self.div_apu_skip_state = DIV_APU_SKIP_INACTIVE;
                self.ch1_timer = 0;
                self.ch2_timer = 0;
                self.ch3_timer = 0;
                self.ch4_timer = 0;
                self.reset_noise_divider_state();
                self.ch1_output_delay = 0;
                self.ch2_output_delay = 0;
                self.ch1_output_suppressed = false;
                self.ch2_output_suppressed = false;
                self.ch1_just_reloaded = false;
                self.ch2_just_reloaded = false;
                self.ch1_sweep_pending_disable_delay = 0;
                self.ch1_sweep_trigger_visibility_delay = 0;
                self.ch3_output_delay = 0;
                self.ch3_restart_pending = false;
                self.ch3_wave_access_window = 0;
                self.ch3_wave_access_index = 0;
                self.ch1_duty_pos = 0;
                self.ch2_duty_pos = 0;
                self.ch1_current_duty = 0;
                self.ch2_current_duty = 0;
                self.ch3_wave_pos = 0;
                self.ch4_lfsr = 0x7FFF;
                self.sample_cycle_accum = 0.0;
                self.sample_buffer.clear();
                self.debug_capture_cycle_accum = 0;

                for history in &mut self.channel_debug_history {
                    history.clear();
                }
                self.master_debug_history.clear();
                self.channel_muted = [false; 4];
                for channel in &mut self.channels {
                    channel.enabled = false;
                    channel.length_enabled = false;
                    channel.length_counter = 0;
                    channel.envelope_zero_period_arm = false;
                    channel.envelope_forced_tick_delay = 0;
                }
                if let Some(length_counters) = length_counters {
                    for (channel, length_counter) in self.channels.iter_mut().zip(length_counters) {
                        channel.length_counter = length_counter;
                    }
                }
            } else {
                if !self.powered() {
                    self.reset_noise_divider_state();
                    self.pulse_noise_cycle_accum = if self.cgb_double_speed { 1 } else { 0 };
                    self.wave_cycle_accum = 0;
                    self.noise_cycle_accum = 0;
                }
                self.nr52 |= 0x80;
                self.update_nr52_status();
            }
            return;
        }

        if !self.powered() {
            if (WAVE_RAM_START..=WAVE_RAM_END).contains(&addr) {
                self.write_wave_ram_cpu(addr, value);
            } else if !self.cgb_hardware && matches!(addr, NR11 | NR21 | NR31 | NR41) {
                self.maybe_write_length(addr, value);
            }
            return;
        }

        match addr {
            NR10..=NR51 => {
                let old_value = self.regs[(addr - NR10) as usize];
                self.regs[(addr - NR10) as usize] = value;
                self.maybe_write_length(addr, value);
                self.maybe_write_length_enable(addr, value);
                self.maybe_write_sweep(addr, value);
                self.maybe_write_envelope(addr, value, old_value);
                self.maybe_apply_square_duty_write(addr, value);
                self.maybe_apply_square_frequency_write(addr, value, old_value);
                self.maybe_apply_wave_frequency_write(addr);
                self.maybe_apply_noise_frequency_write(addr, value, old_value);
                self.maybe_apply_dac_gate(addr);

                if value & 0x80 != 0
                    && let Some((channel_index, channel_mask)) = trigger_channel(addr)
                {
                    let was_enabled = self.channels[channel_index].enabled;
                    self.channels[channel_index].enabled = self.channel_dac_enabled(channel_index);
                    self.reset_channel_runtime(channel_index, was_enabled);
                    if channel_index == 0 {
                        self.init_sweep_on_trigger();
                    }
                    if uses_envelope(channel_index) {
                        self.channels[channel_index].envelope_volume =
                            envelope_initial_volume(self.regs[envelope_reg_index(channel_index)]);
                        self.channels[channel_index].envelope_timer =
                            self.channels[channel_index].envelope_period;
                        self.channels[channel_index].envelope_zero_period_arm = false;
                        self.channels[channel_index].envelope_forced_tick_delay = 0;
                    }
                    if self.channels[channel_index].length_counter == 0 {
                        self.channels[channel_index].length_counter =
                            channel_max_length(channel_index);
                        if self.channels[channel_index].length_enabled
                            && self.frame_seq_step_is_odd()
                        {
                            self.channels[channel_index].length_counter -= 1;
                        }
                    }
                    self.nr52 |= channel_mask;
                    self.update_nr52_status();
                }
            }
            WAVE_RAM_START..=WAVE_RAM_END => {
                self.write_wave_ram_cpu(addr, value);
            }
            _ => {}
        }
    }

    pub(in crate::hardware) fn powered(&self) -> bool {
        (self.nr52 & 0x80) != 0
    }

    pub fn regs_snapshot(&self) -> [u8; 0x17] {
        self.regs
    }

    pub fn wave_ram_snapshot(&self) -> [u8; 0x10] {
        self.wave_ram
    }

    pub fn nr52_raw(&self) -> u8 {
        self.nr52
    }

    pub fn channel_debug_samples(&self, channel: usize) -> &[f32; super::DEBUG_SAMPLE_HISTORY_LEN] {
        &self.channel_debug_history[channel.min(3)].samples
    }

    pub fn channel_debug_samples_ordered(
        &self,
        channel: usize,
    ) -> [f32; super::DEBUG_SAMPLE_HISTORY_LEN] {
        self.channel_debug_history[channel.min(3)].ordered()
    }

    pub fn master_debug_samples_ordered(&self) -> [f32; super::DEBUG_SAMPLE_HISTORY_LEN] {
        self.master_debug_history.ordered()
    }

    pub fn channel_mutes(&self) -> [bool; 4] {
        self.channel_muted
    }

    pub fn set_channel_mutes(&mut self, mutes: [bool; 4]) {
        self.channel_muted = mutes;
    }

    pub fn square_period_t_cycles(&self, freq: u16) -> u64 {
        let base = 2048u16.saturating_sub(freq.max(1));
        u64::from(base.max(1)) * 4
    }

    pub(super) fn square_trigger_phase_delay(&self) -> u64 {
        if self.cgb_double_speed {
            (2 + 4 - (self.pulse_noise_cycle_accum & 0x03)) & 0x03
        } else {
            self.pulse_noise_cycle_accum
        }
    }

    fn advance_channel_timer_clocks(&mut self, t_cycles: u64, powered: bool) {
        self.pulse_noise_cycle_accum = self.pulse_noise_cycle_accum.wrapping_add(t_cycles);
        while self.pulse_noise_cycle_accum >= 4 {
            self.pulse_noise_cycle_accum -= 4;
            if powered {
                self.advance_square_channel(0, 4);
                self.advance_square_channel(1, 4);
            }
        }

        if powered {
            self.advance_noise_clock(t_cycles);
        }

        self.wave_cycle_accum = self.wave_cycle_accum.wrapping_add(t_cycles);
        while self.wave_cycle_accum >= 2 {
            self.wave_cycle_accum -= 2;
            if powered {
                self.advance_wave_channel(2);
            }
        }
    }
}

fn read_mask(addr: u16) -> u8 {
    match addr {
        NR10 => NR10_READ_MASK,
        NR11 => NR11_READ_MASK,
        NR12 => NR12_READ_MASK,
        NR13 => NR13_READ_MASK,
        NR14 => NR14_READ_MASK,
        0xFF15 => NR15_READ_MASK,
        NR21 => NR21_READ_MASK,
        NR22 => NR22_READ_MASK,
        NR23 => NR23_READ_MASK,
        NR24 => NR24_READ_MASK,
        NR30 => NR30_READ_MASK,
        NR31 => NR31_READ_MASK,
        NR32 => NR32_READ_MASK,
        NR33 => NR33_READ_MASK,
        NR34 => NR34_READ_MASK,
        0xFF1F => NR35_READ_MASK,
        NR41 => NR41_READ_MASK,
        NR42 => NR42_READ_MASK,
        NR43 => NR43_READ_MASK,
        NR44 => NR44_READ_MASK,
        NR50 => NR50_READ_MASK,
        NR51 => NR51_READ_MASK,
        _ => 0xFF,
    }
}

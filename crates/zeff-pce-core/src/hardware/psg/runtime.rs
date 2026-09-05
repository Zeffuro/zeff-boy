use super::*;

#[derive(Clone, Copy, Default)]
struct InternalClockAdvance {
    mixer_changed: bool,
    #[cfg(feature = "profiling")]
    source_examinations: u8,
    #[cfg(feature = "profiling")]
    source_transitions: u8,
}

impl InternalClockAdvance {
    #[inline]
    fn examine_source(&mut self, before: u8, after: u8) {
        #[cfg(feature = "profiling")]
        {
            self.source_examinations += 1;
        }
        if before != after {
            self.mixer_changed = true;
            #[cfg(feature = "profiling")]
            {
                self.source_transitions += 1;
            }
        }
    }
}

impl HuC6280Psg {
    pub fn drain_audio_samples_into(&mut self, output: &mut Vec<f32>) {
        if self.sample_generation_enabled {
            self.resampler.flush(&mut self.audio_samples);
        }
        output.clear();
        output.extend(
            self.audio_samples
                .drain(..)
                .map(|sample| f32::from(sample) / 32_768.0),
        );
    }

    pub fn advance_master_ticks(&mut self, master_ticks: u64) {
        let previous_remainder = u64::from(self.master_tick_remainder);
        let total = previous_remainder + master_ticks;
        let internal_clocks = total / PSG_INTERNAL_MASTER_CLOCK_DIVISOR
            - previous_remainder / PSG_INTERNAL_MASTER_CLOCK_DIVISOR;
        self.master_tick_remainder = (total % PSG_MASTER_CLOCK_DIVISOR) as u8;
        let mut oscillator_clock = previous_remainder >= PSG_INTERNAL_MASTER_CLOCK_DIVISOR;
        let mut pending_resampler_clocks = 0;
        for _ in 0..internal_clocks {
            oscillator_clock = !oscillator_clock;
            let advance = self.advance_internal_clock_state(!oscillator_clock);
            if self.sample_generation_enabled {
                pending_resampler_clocks += 1;
                if advance.mixer_changed {
                    self.resampler
                        .advance_clocks(pending_resampler_clocks, &mut self.audio_samples);
                    pending_resampler_clocks = 0;
                    self.refresh_mixer_output();
                }
            }
        }
        if self.sample_generation_enabled {
            self.resampler
                .advance_clocks(pending_resampler_clocks, &mut self.audio_samples);
        }
    }

    #[cfg(feature = "profiling")]
    pub(crate) fn advance_master_ticks_profiled(
        &mut self,
        master_ticks: u64,
        profiling: &mut crate::hardware::profiling::PceProfiling,
    ) {
        profiling.snapshot.psg_advance_calls += 1;
        profiling.snapshot.psg_master_ticks += master_ticks;
        let previous_remainder = u64::from(self.master_tick_remainder);
        let total = previous_remainder + master_ticks;
        let internal_clocks = total / PSG_INTERNAL_MASTER_CLOCK_DIVISOR
            - previous_remainder / PSG_INTERNAL_MASTER_CLOCK_DIVISOR;
        profiling.snapshot.psg_internal_clocks += internal_clocks;
        self.master_tick_remainder = (total % PSG_MASTER_CLOCK_DIVISOR) as u8;
        let mut oscillator_clock = previous_remainder >= PSG_INTERNAL_MASTER_CLOCK_DIVISOR;
        let mut pending_resampler_clocks = 0;
        for _ in 0..internal_clocks {
            oscillator_clock = !oscillator_clock;
            if !oscillator_clock {
                profiling.snapshot.psg_oscillator_clocks += 1;
            }
            let advance = self.advance_internal_clock_state(!oscillator_clock);
            profiling.snapshot.psg_mixer_source_examinations +=
                u64::from(advance.source_examinations);
            profiling.snapshot.psg_mixer_source_transitions +=
                u64::from(advance.source_transitions);
            if self.sample_generation_enabled {
                pending_resampler_clocks += 1;
                if !advance.mixer_changed {
                    continue;
                }
                profiling.snapshot.psg_mix_scans += 1;
                self.resampler
                    .advance_clocks(pending_resampler_clocks, &mut self.audio_samples);
                pending_resampler_clocks = 0;
                self.refresh_mixer_output();
            }
        }
        if self.sample_generation_enabled {
            self.resampler
                .advance_clocks(pending_resampler_clocks, &mut self.audio_samples);
        }
    }

    #[cfg(test)]
    pub(in super::super) const fn master_tick_remainder(&self) -> u8 {
        self.master_tick_remainder
    }

    #[cfg(test)]
    pub(in super::super) const fn resampler_clock(&self) -> u32 {
        self.resampler.clocks
    }

    #[cfg(test)]
    pub(in super::super) const fn resampler_levels(&self) -> (i32, i32) {
        (self.resampler.left_level, self.resampler.right_level)
    }

    #[cfg(test)]
    pub(in super::super) const fn debug_capture_phase(&self) -> u64 {
        self.debug_capture_phase
    }

    #[cfg(test)]
    pub(in super::super) const fn gain_scan_state(&self) -> (bool, bool, u16) {
        (
            self.gain_scan_active,
            self.gain_scan_queued,
            self.gain_scan_clock,
        )
    }

    #[inline]
    pub const fn read_port(&self, _port: PsgPort) -> u8 {
        PSG_UNAVAILABLE_READ_VALUE
    }

    pub fn write_port(&mut self, port: PsgPort, value: u8) {
        let refresh_mixer = match port.offset() {
            0 => {
                self.selected_channel = value & 7;
                false
            }
            1 => {
                self.main_amplitude = value;
                self.queue_gain_scan();
                false
            }
            2 => {
                self.with_selected_channel(|channel| channel.write_frequency_low(value));
                false
            }
            3 => {
                self.with_selected_channel(|channel| channel.write_frequency_high(value));
                false
            }
            4 => {
                let valid = self.with_selected_channel(|channel| channel.write_control(value));
                if valid {
                    self.queue_gain_scan();
                }
                valid
            }
            5 => {
                let valid = self.with_selected_channel(|channel| channel.balance = value);
                if valid {
                    self.queue_gain_scan();
                }
                false
            }
            6 => {
                let refresh = self
                    .selected_channel()
                    .is_some_and(|channel| channel.dda_enabled() || channel.key_on());
                self.with_selected_channel(|channel| channel.write_wave_data(value));
                refresh
            }
            7 => {
                if matches!(self.selected_channel, 4 | 5) {
                    let channel = &mut self.channels[usize::from(self.selected_channel)];
                    channel.noise_control = value & NOISE_CONTROL_MASK;
                    true
                } else {
                    false
                }
            }
            8 => {
                self.lfo_frequency = value;
                if self.lfo_active() {
                    self.lfo_counter = lfo_period(self.channels[1].frequency, value);
                    self.lfo_phase_valid = true;
                }
                false
            }
            9 => {
                let was_active = self.lfo_active();
                self.lfo_control = value & LFO_CONTROL_MASK;
                if self.lfo_halted() {
                    self.channels[1].wave_index = 0;
                    self.lfo_counter = PROVISIONAL_LFO_ZERO_PERIOD;
                    self.lfo_phase_valid = false;
                } else if !was_active && self.lfo_active() {
                    self.channels[1].wave_index = 0;
                    self.lfo_counter = lfo_period(self.channels[1].frequency, self.lfo_frequency);
                    self.lfo_phase_valid = true;
                }
                true
            }
            _ => false,
        };
        if refresh_mixer {
            self.refresh_mixer_output();
        }
    }

    pub(super) fn refresh_mixer_output(&mut self) {
        if !self.sample_generation_enabled {
            return;
        }
        let (left, right) = self.mix_output();
        self.resampler.refresh_level(left, right);
    }

    pub(super) fn queue_gain_scan(&mut self) {
        if self.gain_scan_active {
            self.gain_scan_queued = true;
        } else {
            self.gain_scan_active = true;
            self.gain_scan_clock = 0;
        }
    }

    fn advance_internal_clock_state(&mut self, advance_oscillators: bool) -> InternalClockAdvance {
        let gain_changed = self.advance_gain_scan();
        let mut advance = if advance_oscillators && self.sample_generation_enabled {
            self.advance_oscillators_with_mixer_change()
        } else {
            if advance_oscillators {
                self.advance_oscillators();
            }
            InternalClockAdvance::default()
        };
        self.advance_debug_capture();
        advance.mixer_changed |= gain_changed;
        advance
    }

    #[cfg(test)]
    fn advance_internal_clock_scalar(&mut self, advance_oscillators: bool) {
        self.advance_gain_scan();
        if advance_oscillators {
            self.advance_oscillators();
        }
        self.advance_debug_capture();
        if !self.sample_generation_enabled {
            return;
        }
        let (left, right) = self.mix_output();
        self.resampler
            .push_level(left, right, &mut self.audio_samples);
    }

    fn advance_debug_capture(&mut self) {
        if !self.debug_capture_enabled {
            return;
        }
        self.debug_capture_phase += u64::from(PSG_DEBUG_WAVEFORM_RATE_HZ) * PSG_CLOCK_DENOMINATOR;
        if self.debug_capture_phase < PSG_INTERNAL_CLOCK_NUMERATOR {
            return;
        }
        self.debug_capture_phase -= PSG_INTERNAL_CLOCK_NUMERATOR;
        let channels = self.debug_channel_samples();
        for (history, sample) in self.debug_channel_history.iter_mut().zip(channels) {
            history.push(sample);
        }
        let (left, right) = self.mix_output();
        let master = (left + right) as f32 / (2 * MIX_SCALE) as f32;
        self.debug_master_history.push(master.clamp(-1.0, 1.0));
    }

    fn debug_channel_samples(&self) -> [f32; PSG_CHANNEL_COUNT] {
        std::array::from_fn(|index| {
            let channel = &self.channels[index];
            if !channel.key_on() || self.channel_mutes[index] {
                return 0.0;
            }
            let dac = if channel.noise_enabled() {
                if channel.noise_seed & 1 != 0 { 31 } else { 0 }
            } else if channel.dda_enabled() {
                channel.dda_hold()
            } else {
                channel.waveform[usize::from(channel.wave_index)]
            };
            let sample = match self.revision {
                PsgRevision::HuC6280 => i64::from(dac),
                PsgRevision::HuC6280A => i64::from(dac) - 16,
            };
            let left = sample
                * i64::from(ATTENUATION_GAIN[usize::from(channel.effective_left_attenuation)]);
            let right = sample
                * i64::from(ATTENUATION_GAIN[usize::from(channel.effective_right_attenuation)]);
            ((left + right) as f32 / (2 * 31_000_000) as f32).clamp(-1.0, 1.0)
        })
    }

    pub(super) fn clear_debug_sample_history(&mut self) {
        self.debug_capture_phase = 0;
        self.debug_master_history.clear();
        for history in &mut self.debug_channel_history {
            history.clear();
        }
    }

    fn advance_gain_scan(&mut self) -> bool {
        if !self.gain_scan_active {
            return false;
        }
        let mut changed = false;
        self.gain_scan_clock += 1;
        if (self.gain_scan_clock - 1).is_multiple_of(PROVISIONAL_PSG_GAIN_COMPONENT_CLOCKS) {
            let component = (self.gain_scan_clock - 1) / PROVISIONAL_PSG_GAIN_COMPONENT_CLOCKS;
            let channel = usize::from(component / 2);
            if channel < PSG_CHANNEL_COUNT {
                let channel = &self.channels[channel];
                self.attenuation_latch = if component & 1 == 0 {
                    attenuation_slot(self.main_amplitude, channel.amplitude(), channel.balance)
                } else {
                    attenuation_slot(
                        self.main_amplitude >> 4,
                        channel.amplitude(),
                        channel.balance >> 4,
                    )
                };
            }
        }
        if self
            .gain_scan_clock
            .is_multiple_of(PROVISIONAL_PSG_GAIN_COMPONENT_CLOCKS)
        {
            let component = self.gain_scan_clock / PROVISIONAL_PSG_GAIN_COMPONENT_CLOCKS - 1;
            let channel = usize::from(component / 2);
            if channel < PSG_CHANNEL_COUNT {
                if component & 1 == 0 {
                    changed = self.channels[channel].effective_right_attenuation
                        != self.attenuation_latch;
                    self.channels[channel].effective_right_attenuation = self.attenuation_latch;
                } else {
                    changed =
                        self.channels[channel].effective_left_attenuation != self.attenuation_latch;
                    self.channels[channel].effective_left_attenuation = self.attenuation_latch;
                }
            }
        }
        if self.gain_scan_clock == PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS {
            self.gain_scan_clock = 0;
            if self.gain_scan_queued {
                self.gain_scan_queued = false;
            } else {
                self.gain_scan_active = false;
            }
        }
        changed
    }

    fn advance_oscillators_with_mixer_change(&mut self) -> InternalClockAdvance {
        self.advance_oscillators_inner::<true>()
    }

    fn advance_oscillators(&mut self) {
        self.advance_oscillators_inner::<false>();
    }

    fn advance_oscillators_inner<const TRACK_MIXER: bool>(&mut self) -> InternalClockAdvance {
        let mut advance = InternalClockAdvance::default();
        let lfo_active = self.lfo_active();
        let lfo_halted = self.lfo_halted();
        let channel_mutes = self.channel_mutes;
        if !lfo_active {
            self.lfo_phase_valid = false;
        }
        for (index, channel) in self.channels.iter_mut().enumerate() {
            if index >= 4 {
                if channel.noise_counter <= 1 {
                    channel.noise_counter = noise_period(channel.noise_frequency());
                    let seed = channel.noise_seed;
                    let feedback =
                        (seed ^ (seed >> 1) ^ (seed >> 11) ^ (seed >> 12) ^ (seed >> 17)) & 1;
                    channel.noise_seed = (seed >> 1) | (feedback << 17);
                    if TRACK_MIXER
                        && channel.key_on()
                        && !channel_mutes[index]
                        && channel.noise_enabled()
                    {
                        let before = if seed & 1 != 0 { 31 } else { 0 };
                        let after = if channel.noise_seed & 1 != 0 { 31 } else { 0 };
                        advance.examine_source(before, after);
                    }
                } else {
                    channel.noise_counter -= 1;
                }
            }
            if lfo_active && index == 0 {
                continue;
            }
            if lfo_active && index == 1 {
                continue;
            }
            if lfo_halted && index == 1 {
                continue;
            }
            if channel.key_on() && !channel.noise_enabled() && !channel.dda_enabled() {
                if channel.wave_counter <= 1 {
                    let before_index = channel.wave_index;
                    channel.wave_counter = effective_period(channel.frequency);
                    channel.wave_index = channel.wave_index.wrapping_add(1) & 0x1F;
                    if TRACK_MIXER && !channel_mutes[index] {
                        advance.examine_source(
                            channel.waveform[usize::from(before_index)],
                            channel.waveform[usize::from(channel.wave_index)],
                        );
                    }
                } else {
                    channel.wave_counter -= 1;
                }
            }
        }
        if lfo_active {
            let before_indices = [self.channels[0].wave_index, self.channels[1].wave_index];
            self.advance_lfo_tick();
            if TRACK_MIXER {
                for index in 0..=1 {
                    let channel = &self.channels[index];
                    if before_indices[index] != channel.wave_index
                        && channel.key_on()
                        && !channel_mutes[index]
                        && !channel.noise_enabled()
                        && !channel.dda_enabled()
                    {
                        advance.examine_source(
                            channel.waveform[usize::from(before_indices[index])],
                            channel.waveform[usize::from(channel.wave_index)],
                        );
                    }
                }
            }
        }
        advance
    }

    #[cfg(test)]
    pub(in super::super) fn advance_psg_tick(&mut self) {
        self.advance_internal_clock_scalar(false);
        self.advance_internal_clock_scalar(true);
    }

    #[cfg(test)]
    pub(in super::super) fn advance_master_ticks_scalar(&mut self, master_ticks: u64) {
        let previous_remainder = u64::from(self.master_tick_remainder);
        let total = previous_remainder + master_ticks;
        let internal_clocks = total / PSG_INTERNAL_MASTER_CLOCK_DIVISOR
            - previous_remainder / PSG_INTERNAL_MASTER_CLOCK_DIVISOR;
        self.master_tick_remainder = (total % PSG_MASTER_CLOCK_DIVISOR) as u8;
        let mut oscillator_clock = previous_remainder >= PSG_INTERNAL_MASTER_CLOCK_DIVISOR;
        for _ in 0..internal_clocks {
            oscillator_clock = !oscillator_clock;
            self.advance_internal_clock_scalar(!oscillator_clock);
        }
    }

    pub(super) fn mix_output(&self) -> (i64, i64) {
        let mut left = 0_i64;
        let mut right = 0_i64;
        for (index, channel) in self.channels.iter().enumerate() {
            if !channel.key_on() || self.channel_mutes[index] {
                continue;
            }
            let dac = if channel.noise_enabled() {
                if channel.noise_seed & 1 != 0 { 31 } else { 0 }
            } else if channel.dda_enabled() {
                channel.dda_hold()
            } else {
                channel.waveform[usize::from(channel.wave_index)]
            };
            let sample = match self.revision {
                PsgRevision::HuC6280 => i64::from(dac),
                PsgRevision::HuC6280A => i64::from(dac) - 16,
            };
            left += sample
                * i64::from(ATTENUATION_GAIN[usize::from(channel.effective_left_attenuation)]);
            right += sample
                * i64::from(ATTENUATION_GAIN[usize::from(channel.effective_right_attenuation)]);
        }
        (left, right)
    }

    pub(super) fn resampler_at_current_level(&self) -> StereoBlipResampler {
        let (left, right) = self.mix_output();
        StereoBlipResampler::at_level(self.sample_rate, left, right)
    }

    fn advance_lfo_tick(&mut self) {
        if !self.lfo_phase_valid {
            self.channels[1].wave_index = 0;
            self.lfo_counter = lfo_period(self.channels[1].frequency, self.lfo_frequency);
            self.lfo_phase_valid = true;
        }
        let source_sample = self.channels[1].waveform[usize::from(self.channels[1].wave_index)];
        if self.lfo_counter <= 1 {
            self.lfo_counter = lfo_period(self.channels[1].frequency, self.lfo_frequency);
            self.channels[1].wave_index = self.channels[1].wave_index.wrapping_add(1) & 0x1F;
        } else {
            self.lfo_counter -= 1;
        }

        let period = lfo_target_period(
            effective_period(self.channels[0].frequency),
            source_sample,
            self.lfo_depth(),
        );
        let target = &mut self.channels[0];
        if target.wave_counter <= 1 {
            target.wave_counter = period;
            target.wave_index = target.wave_index.wrapping_add(1) & 0x1F;
        } else {
            target.wave_counter -= 1;
        }
    }

    #[inline]
    fn with_selected_channel(&mut self, write: impl FnOnce(&mut PsgChannel)) -> bool {
        if let Some(channel) = self.channels.get_mut(usize::from(self.selected_channel)) {
            write(channel);
            true
        } else {
            false
        }
    }
}

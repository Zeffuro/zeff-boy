use super::*;

impl HuC6280Psg {
    pub(in super::super) fn validate_v1_state(&self) -> anyhow::Result<()> {
        if self.audio_samples.len() > MAX_PSG_STATE_AUDIO_SAMPLES {
            bail!("PC Engine PSG queued audio exceeds its save-state bound");
        }
        if !self.audio_samples.len().is_multiple_of(2) {
            bail!("PC Engine PSG queued audio is not stereo-aligned");
        }
        if !self.sample_generation_enabled && !self.audio_samples.is_empty() {
            bail!("disabled PC Engine PSG has queued audio in save-state");
        }
        Ok(())
    }

    pub(in super::super) const fn runtime_config(
        &self,
    ) -> (u32, bool, [bool; PSG_CHANNEL_COUNT], bool) {
        (
            self.sample_rate,
            self.sample_generation_enabled,
            self.channel_mutes,
            self.debug_capture_enabled,
        )
    }

    pub(in super::super) fn apply_runtime_config(
        &mut self,
        sample_rate: u32,
        sample_generation_enabled: bool,
        channel_mutes: [bool; PSG_CHANNEL_COUNT],
        debug_capture_enabled: bool,
    ) {
        self.set_sample_rate(sample_rate);
        self.set_sample_generation_enabled(sample_generation_enabled);
        self.set_channel_mutes(&channel_mutes);
        self.set_debug_capture_enabled(debug_capture_enabled);
    }

    pub(in super::super) fn write_state(&self, writer: &mut StateWriter) {
        for channel in &self.channels {
            writer.write_u16(channel.frequency);
            writer.write_u8(channel.control);
            writer.write_u8(channel.balance);
            writer.write_bytes(&channel.waveform);
            writer.write_u8(channel.wave_index);
            writer.write_u8(channel.dda_hold);
            writer.write_u8(channel.noise_control);
            writer.write_u32(channel.wave_counter as u32);
            writer.write_u16(channel.noise_counter);
            writer.write_u32(channel.noise_seed);
            writer.write_u8(channel.effective_left_attenuation);
            writer.write_u8(channel.effective_right_attenuation);
        }
        writer.write_u8(self.selected_channel);
        writer.write_u8(self.main_amplitude);
        writer.write_u8(self.lfo_frequency);
        writer.write_u8(self.lfo_control);
        writer.write_u32(self.lfo_counter as u32);
        writer.write_bool(self.lfo_phase_valid);
        writer.write_u16(self.gain_scan_clock);
        writer.write_bool(self.gain_scan_active);
        writer.write_bool(self.gain_scan_queued);
        writer.write_u8(self.attenuation_latch);
        writer.write_u8(self.master_tick_remainder);
        writer.write_u32(self.sample_rate);
        writer.write_bool(self.sample_generation_enabled);
        self.resampler.write_state(writer);
        writer.write_u32(self.audio_samples.len() as u32);
        for sample in &self.audio_samples {
            writer.write_u16(*sample as u16);
        }
    }

    pub(in super::super) fn read_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> anyhow::Result<()> {
        let target_generation_enabled = self.sample_generation_enabled;
        let mut channels = [const { PsgChannel::new() }; PSG_CHANNEL_COUNT];
        for channel in &mut channels {
            channel.frequency = reader.read_u16()?;
            if channel.frequency > 0x0FFF {
                bail!("invalid PSG channel frequency in save-state");
            }
            channel.control = reader.read_u8()?;
            if channel.control & !CHANNEL_CONTROL_MASK != 0 {
                bail!("invalid PSG channel control in save-state");
            }
            channel.balance = reader.read_u8()?;
            reader.read_exact(&mut channel.waveform)?;
            if channel.waveform.iter().any(|&sample| sample > 0x1F) {
                bail!("invalid PSG waveform sample in save-state");
            }
            channel.wave_index = reader.read_u8()?;
            channel.dda_hold = reader.read_u8()?;
            if channel.wave_index > 0x1F || channel.dda_hold > 0x1F {
                bail!("invalid PSG waveform cursor or DDA value in save-state");
            }
            channel.noise_control = reader.read_u8()?;
            if channel.noise_control & !NOISE_CONTROL_MASK != 0 {
                bail!("invalid PSG noise control in save-state");
            }
            channel.wave_counter = reader.read_u32()? as i32;
            channel.noise_counter = reader.read_u16()?;
            channel.noise_seed = reader.read_u32()?;
            if channel.noise_seed == 0 || channel.noise_seed > 0x3_FFFF {
                bail!("invalid PSG noise seed in save-state");
            }
            channel.effective_left_attenuation = reader.read_u8()?;
            channel.effective_right_attenuation = reader.read_u8()?;
            if channel.effective_left_attenuation > 31 || channel.effective_right_attenuation > 31 {
                bail!("invalid PSG attenuation in save-state");
            }
        }

        let selected_channel = reader.read_u8()?;
        if selected_channel > 7 {
            bail!("invalid PSG selected channel in save-state: {selected_channel}");
        }
        let main_amplitude = reader.read_u8()?;
        let lfo_frequency = reader.read_u8()?;
        let lfo_control = reader.read_u8()?;
        if lfo_control & !LFO_CONTROL_MASK != 0 {
            bail!("invalid PSG LFO control in save-state");
        }
        let lfo_counter = reader.read_u32()? as i32;
        let lfo_phase_valid = reader.read_bool()?;
        let gain_scan_clock = reader.read_u16()?;
        if gain_scan_clock >= PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS {
            bail!("invalid PSG gain-scan clock in save-state: {gain_scan_clock}");
        }
        let gain_scan_active = reader.read_bool()?;
        let gain_scan_queued = reader.read_bool()?;
        if gain_scan_queued && !gain_scan_active {
            bail!("queued PSG gain scan is inactive in save-state");
        }
        let attenuation_latch = reader.read_u8()?;
        if attenuation_latch > 31 {
            bail!("invalid PSG attenuation latch in save-state: {attenuation_latch}");
        }
        let master_tick_remainder = reader.read_u8()?;
        if master_tick_remainder >= PSG_MASTER_CLOCK_DIVISOR as u8 {
            bail!("invalid PSG master-clock remainder in save-state: {master_tick_remainder}");
        }
        let saved_sample_rate = reader.read_u32()?;
        if saved_sample_rate != self.sample_rate {
            bail!(
                "PC Engine PSG save-state sample rate mismatch: state is {saved_sample_rate} Hz, destination is {} Hz",
                self.sample_rate
            );
        }
        let saved_generation_enabled = reader.read_bool()?;
        let saved_resampler = StereoBlipResampler::read_state(saved_sample_rate, reader)?;
        let audio_sample_count = reader.read_u32()? as usize;
        if audio_sample_count > MAX_PSG_STATE_AUDIO_SAMPLES {
            bail!("PSG queued-audio sample count exceeds save-state bound: {audio_sample_count}");
        }
        if !audio_sample_count.is_multiple_of(2) {
            bail!("PSG queued audio is not stereo-aligned in save-state");
        }
        if !saved_generation_enabled && audio_sample_count != 0 {
            bail!("disabled PSG has queued audio in save-state");
        }
        let mut saved_audio_samples = Vec::with_capacity(audio_sample_count);
        for _ in 0..audio_sample_count {
            saved_audio_samples.push(reader.read_u16()? as i16);
        }

        self.channels = channels;
        self.selected_channel = selected_channel;
        self.main_amplitude = main_amplitude;
        self.lfo_frequency = lfo_frequency;
        self.lfo_control = lfo_control;
        self.lfo_counter = lfo_counter;
        self.lfo_phase_valid = lfo_phase_valid;
        self.gain_scan_clock = gain_scan_clock;
        self.gain_scan_active = gain_scan_active;
        self.gain_scan_queued = gain_scan_queued;
        self.attenuation_latch = attenuation_latch;
        self.master_tick_remainder = master_tick_remainder;
        if target_generation_enabled && saved_generation_enabled {
            self.resampler = saved_resampler;
            self.audio_samples = saved_audio_samples;
            self.refresh_mixer_output();
        } else if target_generation_enabled {
            self.resampler = self.resampler_at_current_level();
            self.audio_samples.clear();
        } else {
            self.resampler = StereoBlipResampler::new(self.sample_rate);
            self.audio_samples.clear();
        }
        self.clear_debug_sample_history();
        Ok(())
    }
}

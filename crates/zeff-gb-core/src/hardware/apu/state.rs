use super::Apu;
use crate::hardware::types::constants::*;
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

const COMPLETE_RUNTIME_STATE_FORMAT_VERSION: u32 = 5;
const MAX_NOISE_COUNTER_COUNTDOWN: u64 = 32;

impl Apu {
    pub fn apply_dmg_post_boot_io(&mut self) {
        self.regs.fill(0);
        self.wave_ram.fill(0);

        self.regs[(NR11 - NR10) as usize] = 0x80;
        self.regs[(NR12 - NR10) as usize] = 0xF3;
        self.regs[(NR50 - NR10) as usize] = 0x77;
        self.regs[(NR51 - NR10) as usize] = 0xF3;
        self.ch1_current_duty = (self.regs[(NR11 - NR10) as usize] >> 6) & 0x03;
        self.ch2_current_duty = (self.regs[(NR21 - NR10) as usize] >> 6) & 0x03;
        self.ch1_output_suppressed = false;
        self.ch2_output_suppressed = false;
        self.ch1_just_reloaded = false;
        self.ch2_just_reloaded = false;
        self.ch1_sweep_pending_disable_delay = 0;
        self.ch1_sweep_trigger_visibility_delay = 0;
        self.div_apu_phase_high = false;

        for channel in &mut self.channels {
            channel.enabled = false;
            channel.length_enabled = false;
            channel.length_counter = 0;
            channel.sweep_period = 0;
            channel.sweep_negate = false;
            channel.sweep_negate_used = false;
            channel.sweep_shift = 0;
            channel.sweep_timer = 0;
            channel.sweep_shadow_freq = 0;
            channel.sweep_enabled = false;
            channel.envelope_period = 0;
            channel.envelope_increase = false;
            channel.envelope_volume = 0;
            channel.envelope_timer = 0;
            channel.envelope_zero_period_arm = false;
            channel.envelope_forced_tick_delay = 0;
        }

        self.channels[0].enabled = true;
        self.channels[0].length_counter = 64;
        self.channels[0].envelope_volume = 0x0F;
        self.nr52 = 0x81;
    }

    pub fn apply_bess_io(&mut self, io_regs: &[u8]) {
        // NR52 (FF26) - master enable only, write first per BESS spec
        self.nr52 = io_regs[0x26] & 0x80;
        // NR10–NR51 (FF10–FF25) → regs[0..22]
        self.regs[..0x17].copy_from_slice(&io_regs[0x10..0x27]);
        self.ch1_current_duty = (self.regs[(NR11 - NR10) as usize] >> 6) & 0x03;
        self.ch2_current_duty = (self.regs[(NR21 - NR10) as usize] >> 6) & 0x03;
        self.ch1_output_suppressed = self.ch1_output_delay != 0;
        self.ch2_output_suppressed = self.ch2_output_delay != 0;
        self.ch1_just_reloaded = false;
        self.ch2_just_reloaded = false;
        self.ch1_sweep_pending_disable_delay = 0;
        self.ch1_sweep_trigger_visibility_delay = 0;
        self.div_apu_phase_high = false;
        for channel in &mut self.channels {
            channel.envelope_zero_period_arm = false;
            channel.envelope_forced_tick_delay = 0;
        }
        // Wave RAM (FF30–FF3F)
        self.wave_ram.copy_from_slice(&io_regs[0x30..0x40]);
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        self.write_state_for_version(writer, crate::save_state::SAVE_STATE_FORMAT_VERSION);
    }

    fn write_state_for_version(&self, writer: &mut StateWriter, format_version: u32) {
        writer.write_bytes(&self.regs);
        writer.write_bytes(&self.wave_ram);
        writer.write_u8(self.nr52);
        for ch in &self.channels {
            writer.write_bool(ch.enabled);
            writer.write_bool(ch.length_enabled);
            writer.write_u16(ch.length_counter);
            writer.write_u8(ch.sweep_period);
            writer.write_bool(ch.sweep_negate);
            writer.write_bool(ch.sweep_negate_used);
            writer.write_u8(ch.sweep_shift);
            writer.write_u8(ch.sweep_timer);
            writer.write_u16(ch.sweep_shadow_freq);
            writer.write_bool(ch.sweep_enabled);
            writer.write_u8(ch.envelope_period);
            writer.write_bool(ch.envelope_increase);
            writer.write_u8(ch.envelope_volume);
            writer.write_u8(ch.envelope_timer);
        }
        writer.write_u64(self.frame_seq_cycle_accum);
        writer.write_u8(self.frame_seq_step);
        writer.write_u64(self.ch1_timer);
        writer.write_u64(self.ch2_timer);
        writer.write_u64(self.ch3_timer);
        writer.write_u64(self.ch4_timer);
        writer.write_u8(self.ch1_duty_pos);
        writer.write_u8(self.ch2_duty_pos);
        writer.write_u8(self.ch3_wave_pos);
        writer.write_u16(self.ch4_lfsr);

        if format_version < COMPLETE_RUNTIME_STATE_FORMAT_VERSION {
            return;
        }

        for channel in &self.channels {
            writer.write_bool(channel.envelope_zero_period_arm);
            writer.write_u8(channel.envelope_forced_tick_delay);
        }
        writer.write_bool(self.div_apu_phase_high);
        writer.write_u8(self.div_apu_skip_state);
        writer.write_u64(self.pulse_noise_cycle_accum);
        writer.write_u64(self.wave_cycle_accum);
        writer.write_u64(self.noise_cycle_accum);
        writer.write_u64(self.ch1_output_delay);
        writer.write_u64(self.ch2_output_delay);
        writer.write_bool(self.ch1_output_suppressed);
        writer.write_bool(self.ch2_output_suppressed);
        writer.write_bool(self.ch1_just_reloaded);
        writer.write_bool(self.ch2_just_reloaded);
        writer.write_u64(self.ch1_sweep_pending_disable_delay);
        writer.write_u64(self.ch1_sweep_trigger_visibility_delay);
        writer.write_u64(self.ch3_output_delay);
        writer.write_bool(self.ch3_restart_pending);
        writer.write_u64(self.ch3_wave_access_window);
        writer.write_u64(self.ch3_wave_access_index as u64);
        writer.write_u8(self.ch1_current_duty);
        writer.write_u8(self.ch2_current_duty);
        writer.write_u16(self.ch4_counter);
        writer.write_u64(self.ch4_counter_countdown);
        writer.write_u8(self.ch4_alignment);
        writer.write_bool(self.ch4_counter_active);
        writer.write_bool(self.ch4_background_counter_active);
        writer.write_bool(self.ch4_did_step_counter);
        writer.write_bool(self.ch4_countdown_reloaded);
    }

    pub fn read_state(reader: &mut StateReader<'_>, format_version: u32) -> Result<Self> {
        let mut apu = Self::new();
        reader.read_exact(&mut apu.regs)?;
        reader.read_exact(&mut apu.wave_ram)?;
        apu.nr52 = reader.read_u8()?;
        for ch in &mut apu.channels {
            ch.enabled = reader.read_bool()?;
            ch.length_enabled = reader.read_bool()?;
            ch.length_counter = reader.read_u16()?;
            ch.sweep_period = reader.read_u8()?;
            ch.sweep_negate = reader.read_bool()?;
            ch.sweep_negate_used = reader.read_bool()?;
            ch.sweep_shift = reader.read_u8()?;
            ch.sweep_timer = reader.read_u8()?;
            ch.sweep_shadow_freq = reader.read_u16()?;
            ch.sweep_enabled = reader.read_bool()?;
            ch.envelope_period = reader.read_u8()?;
            ch.envelope_increase = reader.read_bool()?;
            ch.envelope_volume = reader.read_u8()?;
            ch.envelope_timer = reader.read_u8()?;
            ch.envelope_zero_period_arm = false;
            ch.envelope_forced_tick_delay = 0;
        }
        apu.frame_seq_cycle_accum = reader.read_u64()?;
        apu.frame_seq_step = reader.read_u8()?;
        apu.ch1_timer = reader.read_u64()?;
        apu.ch2_timer = reader.read_u64()?;
        apu.ch3_timer = reader.read_u64()?;
        apu.ch4_timer = reader.read_u64()?;
        apu.ch1_duty_pos = reader.read_u8()?;
        apu.ch2_duty_pos = reader.read_u8()?;
        apu.ch3_wave_pos = reader.read_u8()?;
        apu.ch4_lfsr = reader.read_u16()?;

        if format_version >= COMPLETE_RUNTIME_STATE_FORMAT_VERSION {
            for channel in &mut apu.channels {
                channel.envelope_zero_period_arm = reader.read_bool()?;
                channel.envelope_forced_tick_delay = reader.read_u8()?;
            }
            apu.div_apu_phase_high = reader.read_bool()?;
            apu.div_apu_skip_state = reader.read_u8()?;
            if apu.div_apu_skip_state > super::DIV_APU_SKIP_REPLAY_FIRST_CLOCK {
                anyhow::bail!(
                    "invalid APU DIV skip state in save-state: {}",
                    apu.div_apu_skip_state
                );
            }
            apu.pulse_noise_cycle_accum = reader.read_u64()?;
            apu.wave_cycle_accum = reader.read_u64()?;
            apu.noise_cycle_accum = reader.read_u64()?;
            apu.ch1_output_delay = reader.read_u64()?;
            apu.ch2_output_delay = reader.read_u64()?;
            apu.ch1_output_suppressed = reader.read_bool()?;
            apu.ch2_output_suppressed = reader.read_bool()?;
            apu.ch1_just_reloaded = reader.read_bool()?;
            apu.ch2_just_reloaded = reader.read_bool()?;
            apu.ch1_sweep_pending_disable_delay = reader.read_u64()?;
            apu.ch1_sweep_trigger_visibility_delay = reader.read_u64()?;
            apu.ch3_output_delay = reader.read_u64()?;
            apu.ch3_restart_pending = reader.read_bool()?;
            apu.ch3_wave_access_window = reader.read_u64()?;
            apu.ch3_wave_access_index = usize::try_from(reader.read_u64()?)
                .map_err(|_| anyhow::anyhow!("APU wave access index does not fit usize"))?;
            apu.ch1_current_duty = reader.read_u8()?;
            apu.ch2_current_duty = reader.read_u8()?;
            if apu.ch3_wave_access_index >= apu.wave_ram.len() {
                anyhow::bail!(
                    "invalid APU wave access index in save-state: {}",
                    apu.ch3_wave_access_index
                );
            }
            if apu.ch1_current_duty >= 4 || apu.ch2_current_duty >= 4 {
                anyhow::bail!(
                    "invalid APU duty in save-state: ch1={}, ch2={}",
                    apu.ch1_current_duty,
                    apu.ch2_current_duty
                );
            }
            apu.ch4_counter = reader.read_u16()?;
            apu.ch4_counter_countdown = reader.read_u64()?;
            apu.ch4_alignment = reader.read_u8()?;
            apu.ch4_counter_active = reader.read_bool()?;
            apu.ch4_background_counter_active = reader.read_bool()?;
            apu.ch4_did_step_counter = reader.read_bool()?;
            apu.ch4_countdown_reloaded = reader.read_bool()?;
            if apu.pulse_noise_cycle_accum >= 4
                || apu.wave_cycle_accum >= 2
                || apu.noise_cycle_accum >= 2
            {
                anyhow::bail!("invalid APU channel clock phase in save-state");
            }
            if apu.ch4_counter > 0x3FFF || apu.ch4_counter_countdown > MAX_NOISE_COUNTER_COUNTDOWN {
                anyhow::bail!("invalid APU noise counter state in save-state");
            }
        } else {
            apu.ch1_current_duty = (apu.regs[(NR11 - NR10) as usize] >> 6) & 0x03;
            apu.ch2_current_duty = (apu.regs[(NR21 - NR10) as usize] >> 6) & 0x03;
            apu.ch1_output_suppressed = apu.ch1_output_delay != 0;
            apu.ch2_output_suppressed = apu.ch2_output_delay != 0;
            apu.ch1_just_reloaded = false;
            apu.ch2_just_reloaded = false;
            apu.ch1_sweep_pending_disable_delay = 0;
            apu.ch1_sweep_trigger_visibility_delay = 0;
            apu.div_apu_phase_high = false;
        }

        apu.sample_buffer.clear();
        apu.sample_cycle_accum = 0.0;
        apu.debug_capture_enabled = false;
        apu.sample_generation_enabled = true;
        apu.debug_capture_cycle_accum = 0;
        for history in &mut apu.channel_debug_history {
            history.clear();
        }
        apu.master_debug_history.clear();
        apu.channel_muted = [false; 4];
        Ok(apu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(apu: &Apu, format_version: u32) -> Vec<u8> {
        let mut writer = StateWriter::new();
        apu.write_state_for_version(&mut writer, format_version);
        writer.into_bytes()
    }

    fn decode(bytes: &[u8], format_version: u32) -> Apu {
        let mut reader = StateReader::new(bytes);
        let apu = Apu::read_state(&mut reader, format_version).expect("APU state should decode");
        assert!(reader.is_exhausted());
        apu
    }

    fn active_pipeline() -> Apu {
        let mut apu = Apu::new();
        apu.write(NR52, 0x80);
        apu.write(NR50, 0x77);
        apu.write(NR51, 0xFF);

        apu.write(NR10, 0x16);
        apu.write(NR11, 0x80);
        apu.write(NR12, 0xF3);
        apu.write(NR13, 0xFC);
        apu.write(NR14, 0x87);

        apu.write(NR21, 0x40);
        apu.write(NR22, 0xA2);
        apu.write(NR23, 0xF8);
        apu.write(NR24, 0x87);

        for offset in 0..16u16 {
            apu.write(WAVE_RAM_START + offset, (offset as u8).wrapping_mul(0x11));
        }
        apu.write(NR30, 0x80);
        apu.write(NR32, 0x20);
        apu.write(NR33, 0xFA);
        apu.write(NR34, 0x87);

        apu.write(NR41, 0x20);
        apu.write(NR42, 0xF2);
        apu.write(NR43, 0x13);
        apu.write(NR44, 0x80);

        apu.clock_div_apu_secondary_event();
        apu.channels[0].envelope_zero_period_arm = true;
        apu.channels[1].envelope_forced_tick_delay = 2;
        apu.step(3);
        apu.sample_buffer.clear();
        apu
    }

    #[test]
    fn complete_runtime_state_roundtrip_continues_all_channel_pipelines() {
        let mut original = active_pipeline();
        original.sample_cycle_accum = 0.0;
        assert_ne!(original.pulse_noise_cycle_accum, 0);
        assert_ne!(original.wave_cycle_accum, 0);
        assert_ne!(original.noise_cycle_accum, 0);
        assert_ne!(original.ch1_output_delay, 0);
        assert_ne!(original.ch3_output_delay, 0);
        assert_ne!(original.ch4_counter_countdown, 0);
        assert!(original.div_apu_phase_high);

        let bytes = encode(&original, COMPLETE_RUNTIME_STATE_FORMAT_VERSION);
        let mut restored = decode(&bytes, COMPLETE_RUNTIME_STATE_FORMAT_VERSION);
        assert_eq!(
            encode(&restored, COMPLETE_RUNTIME_STATE_FORMAT_VERSION),
            bytes
        );

        for (index, cycles) in [1, 2, 3, 7, 31, 64, 255, 4097, 8192]
            .into_iter()
            .enumerate()
        {
            if index == 5 {
                original.clock_div_apu();
                restored.clock_div_apu();
            }
            original.step(cycles);
            restored.step(cycles);
            assert_eq!(
                encode(&restored, COMPLETE_RUNTIME_STATE_FORMAT_VERSION),
                encode(&original, COMPLETE_RUNTIME_STATE_FORMAT_VERSION),
                "APU state diverged after continuation slice {index}"
            );
            assert_eq!(restored.drain_samples(), original.drain_samples());
        }
    }

    #[test]
    fn legacy_runtime_state_layout_keeps_prior_defaulting_behavior() {
        let original = active_pipeline();
        let bytes = encode(&original, COMPLETE_RUNTIME_STATE_FORMAT_VERSION - 1);
        let restored = decode(&bytes, COMPLETE_RUNTIME_STATE_FORMAT_VERSION - 1);

        assert_eq!(restored.pulse_noise_cycle_accum, 0);
        assert_eq!(restored.wave_cycle_accum, 0);
        assert_eq!(restored.noise_cycle_accum, 0);
        assert_eq!(restored.ch4_counter_countdown, 0);
        assert!(!restored.div_apu_phase_high);
        assert_eq!(
            encode(&restored, COMPLETE_RUNTIME_STATE_FORMAT_VERSION - 1),
            bytes
        );
    }

    #[test]
    fn complete_runtime_state_rejects_invalid_pipeline_counters() {
        let mut apu = active_pipeline();
        apu.ch4_counter_countdown = MAX_NOISE_COUNTER_COUNTDOWN + 1;
        let bytes = encode(&apu, COMPLETE_RUNTIME_STATE_FORMAT_VERSION);
        let mut reader = StateReader::new(&bytes);
        let err = Apu::read_state(&mut reader, COMPLETE_RUNTIME_STATE_FORMAT_VERSION)
            .expect_err("out-of-range noise countdown should be rejected");
        assert!(err.to_string().contains("noise counter state"));
    }
}

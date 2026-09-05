use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::*;

pub(in crate::hardware::apu) const SAVE_STATE_SIZE: usize = 244;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PsgSaveState {
    regs: [u8; 0x17],
    wave_ram: [u8; 0x10],
    nr52: u8,
    channels: [ChannelState; 4],
    frame_seq_cycle_accum: u64,
    frame_seq_step: u8,
    ch1_timer: u64,
    ch2_timer: u64,
    ch3_timer: u64,
    ch4_timer: u64,
    pulse_noise_cycle_accum: u64,
    wave_cycle_accum: u64,
    noise_cycle_accum: u64,
    ch1_output_delay: u64,
    ch2_output_delay: u64,
    ch1_output_suppressed: bool,
    ch2_output_suppressed: bool,
    ch1_just_reloaded: bool,
    ch2_just_reloaded: bool,
    ch1_sweep_pending_disable_delay: u64,
    ch1_sweep_trigger_visibility_delay: u64,
    ch3_output_delay: u64,
    ch3_restart_pending: bool,
    ch1_current_duty: u8,
    ch2_current_duty: u8,
    ch1_duty_pos: u8,
    ch2_duty_pos: u8,
    ch3_wave_pos: u8,
    ch4_lfsr: u16,
    ch4_counter: u16,
    ch4_counter_countdown: u64,
    ch4_alignment: u8,
    ch4_counter_active: bool,
    ch4_background_counter_active: bool,
    ch4_did_step_counter: bool,
    ch4_countdown_reloaded: bool,
}

impl PsgSaveState {
    fn capture(psg: &Psg) -> Self {
        Self {
            regs: psg.regs,
            wave_ram: psg.wave_ram,
            nr52: psg.nr52,
            channels: psg.channels,
            frame_seq_cycle_accum: psg.frame_seq_cycle_accum,
            frame_seq_step: psg.frame_seq_step,
            ch1_timer: psg.ch1_timer,
            ch2_timer: psg.ch2_timer,
            ch3_timer: psg.ch3_timer,
            ch4_timer: psg.ch4_timer,
            pulse_noise_cycle_accum: psg.pulse_noise_cycle_accum,
            wave_cycle_accum: psg.wave_cycle_accum,
            noise_cycle_accum: psg.noise_cycle_accum,
            ch1_output_delay: psg.ch1_output_delay,
            ch2_output_delay: psg.ch2_output_delay,
            ch1_output_suppressed: psg.ch1_output_suppressed,
            ch2_output_suppressed: psg.ch2_output_suppressed,
            ch1_just_reloaded: psg.ch1_just_reloaded,
            ch2_just_reloaded: psg.ch2_just_reloaded,
            ch1_sweep_pending_disable_delay: psg.ch1_sweep_pending_disable_delay,
            ch1_sweep_trigger_visibility_delay: psg.ch1_sweep_trigger_visibility_delay,
            ch3_output_delay: psg.ch3_output_delay,
            ch3_restart_pending: psg.ch3_restart_pending,
            ch1_current_duty: psg.ch1_current_duty,
            ch2_current_duty: psg.ch2_current_duty,
            ch1_duty_pos: psg.ch1_duty_pos,
            ch2_duty_pos: psg.ch2_duty_pos,
            ch3_wave_pos: psg.ch3_wave_pos,
            ch4_lfsr: psg.ch4_lfsr,
            ch4_counter: psg.ch4_counter,
            ch4_counter_countdown: psg.ch4_counter_countdown,
            ch4_alignment: psg.ch4_alignment,
            ch4_counter_active: psg.ch4_counter_active,
            ch4_background_counter_active: psg.ch4_background_counter_active,
            ch4_did_step_counter: psg.ch4_did_step_counter,
            ch4_countdown_reloaded: psg.ch4_countdown_reloaded,
        }
    }

    fn reset() -> Self {
        Self {
            ch4_lfsr: 0x7FFF,
            ..Self::default()
        }
    }

    fn write(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.regs);
        writer.write_bytes(&self.wave_ram);
        writer.write_u8(self.nr52);
        for channel in &self.channels {
            write_channel(writer, channel);
        }
        writer.write_u64(self.frame_seq_cycle_accum);
        writer.write_u8(self.frame_seq_step);
        writer.write_u64(self.ch1_timer);
        writer.write_u64(self.ch2_timer);
        writer.write_u64(self.ch3_timer);
        writer.write_u64(self.ch4_timer);
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
        writer.write_u8(self.ch1_current_duty);
        writer.write_u8(self.ch2_current_duty);
        writer.write_u8(self.ch1_duty_pos);
        writer.write_u8(self.ch2_duty_pos);
        writer.write_u8(self.ch3_wave_pos);
        writer.write_u16(self.ch4_lfsr);
        writer.write_u16(self.ch4_counter);
        writer.write_u64(self.ch4_counter_countdown);
        writer.write_u8(self.ch4_alignment);
        writer.write_bool(self.ch4_counter_active);
        writer.write_bool(self.ch4_background_counter_active);
        writer.write_bool(self.ch4_did_step_counter);
        writer.write_bool(self.ch4_countdown_reloaded);
    }

    fn read(reader: &mut StateReader<'_>) -> Result<Self> {
        let mut state = Self::default();
        reader.read_exact(&mut state.regs)?;
        reader.read_exact(&mut state.wave_ram)?;
        state.nr52 = reader.read_u8()?;
        for channel in &mut state.channels {
            *channel = read_channel(reader)?;
        }
        state.frame_seq_cycle_accum = reader.read_u64()?;
        state.frame_seq_step = reader.read_u8()?;
        state.ch1_timer = reader.read_u64()?;
        state.ch2_timer = reader.read_u64()?;
        state.ch3_timer = reader.read_u64()?;
        state.ch4_timer = reader.read_u64()?;
        state.pulse_noise_cycle_accum = reader.read_u64()?;
        state.wave_cycle_accum = reader.read_u64()?;
        state.noise_cycle_accum = reader.read_u64()?;
        state.ch1_output_delay = reader.read_u64()?;
        state.ch2_output_delay = reader.read_u64()?;
        state.ch1_output_suppressed = reader.read_bool()?;
        state.ch2_output_suppressed = reader.read_bool()?;
        state.ch1_just_reloaded = reader.read_bool()?;
        state.ch2_just_reloaded = reader.read_bool()?;
        state.ch1_sweep_pending_disable_delay = reader.read_u64()?;
        state.ch1_sweep_trigger_visibility_delay = reader.read_u64()?;
        state.ch3_output_delay = reader.read_u64()?;
        state.ch3_restart_pending = reader.read_bool()?;
        state.ch1_current_duty = reader.read_u8()?;
        state.ch2_current_duty = reader.read_u8()?;
        state.ch1_duty_pos = reader.read_u8()?;
        state.ch2_duty_pos = reader.read_u8()?;
        state.ch3_wave_pos = reader.read_u8()?;
        state.ch4_lfsr = reader.read_u16()?;
        state.ch4_counter = reader.read_u16()?;
        state.ch4_counter_countdown = reader.read_u64()?;
        state.ch4_alignment = reader.read_u8()?;
        state.ch4_counter_active = reader.read_bool()?;
        state.ch4_background_counter_active = reader.read_bool()?;
        state.ch4_did_step_counter = reader.read_bool()?;
        state.ch4_countdown_reloaded = reader.read_bool()?;
        state.validate()?;
        Ok(state)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.nr52 & !0x8F == 0, "invalid GBA PSG power state");
        let active = self
            .channels
            .iter()
            .enumerate()
            .fold(0, |bits, (index, channel)| {
                bits | (u8::from(channel.enabled) << index)
            });
        ensure!(self.nr52 & 0x0F == active, "invalid GBA PSG channel state");
        ensure!(
            self.nr52 & 0x80 != 0 || active == 0,
            "invalid GBA PSG powered-off state"
        );
        for (index, channel) in self.channels.iter().enumerate() {
            ensure!(
                channel.length_counter <= super::frame_seq::channel_max_length(index),
                "invalid GBA PSG length state"
            );
            ensure!(
                channel.sweep_period <= 7
                    && channel.sweep_shift <= 7
                    && channel.sweep_timer <= 8
                    && channel.sweep_shadow_freq <= 0x07FF,
                "invalid GBA PSG sweep state"
            );
            ensure!(
                channel.envelope_period <= 7
                    && channel.envelope_volume <= 15
                    && channel.envelope_timer <= 8
                    && channel.envelope_forced_tick_delay <= 2,
                "invalid GBA PSG envelope state"
            );
        }
        ensure!(
            self.frame_seq_cycle_accum < FRAME_SEQUENCER_PERIOD_CYCLES && self.frame_seq_step < 8,
            "invalid GBA PSG frame sequencer state"
        );
        ensure!(
            self.pulse_noise_cycle_accum < 4
                && self.wave_cycle_accum < 2
                && self.noise_cycle_accum < 2,
            "invalid GBA PSG clock state"
        );
        ensure!(
            self.ch1_timer <= 8_188
                && self.ch2_timer <= 8_188
                && self.ch3_timer <= 4_094
                && self.ch4_timer == 0
                && self.ch1_output_delay <= 8_199
                && self.ch2_output_delay <= 8_199
                && self.ch3_output_delay <= 4_100,
            "invalid GBA PSG timer state"
        );
        ensure!(
            self.ch1_sweep_pending_disable_delay <= 36
                && self.ch1_sweep_trigger_visibility_delay <= 8,
            "invalid GBA PSG sweep delay"
        );
        ensure!(
            self.ch1_current_duty <= 3
                && self.ch2_current_duty <= 3
                && self.ch1_duty_pos <= 7
                && self.ch2_duty_pos <= 7
                && self.ch3_wave_pos <= 31,
            "invalid GBA PSG phase state"
        );
        ensure!(
            self.ch4_lfsr <= 0x7FFF
                && self.ch4_counter <= 0x3FFF
                && self.ch4_counter_countdown <= 38,
            "invalid GBA PSG noise state"
        );
        Ok(())
    }

    fn apply(self, psg: &mut Psg) {
        psg.regs = self.regs;
        psg.wave_ram = self.wave_ram;
        psg.nr52 = self.nr52;
        psg.channels = self.channels;
        psg.frame_seq_cycle_accum = self.frame_seq_cycle_accum;
        psg.frame_seq_step = self.frame_seq_step;
        psg.ch1_timer = self.ch1_timer;
        psg.ch2_timer = self.ch2_timer;
        psg.ch3_timer = self.ch3_timer;
        psg.ch4_timer = self.ch4_timer;
        psg.pulse_noise_cycle_accum = self.pulse_noise_cycle_accum;
        psg.wave_cycle_accum = self.wave_cycle_accum;
        psg.noise_cycle_accum = self.noise_cycle_accum;
        psg.ch1_output_delay = self.ch1_output_delay;
        psg.ch2_output_delay = self.ch2_output_delay;
        psg.ch1_output_suppressed = self.ch1_output_suppressed;
        psg.ch2_output_suppressed = self.ch2_output_suppressed;
        psg.ch1_just_reloaded = self.ch1_just_reloaded;
        psg.ch2_just_reloaded = self.ch2_just_reloaded;
        psg.ch1_sweep_pending_disable_delay = self.ch1_sweep_pending_disable_delay;
        psg.ch1_sweep_trigger_visibility_delay = self.ch1_sweep_trigger_visibility_delay;
        psg.ch3_output_delay = self.ch3_output_delay;
        psg.ch3_restart_pending = self.ch3_restart_pending;
        psg.ch1_current_duty = self.ch1_current_duty;
        psg.ch2_current_duty = self.ch2_current_duty;
        psg.ch1_duty_pos = self.ch1_duty_pos;
        psg.ch2_duty_pos = self.ch2_duty_pos;
        psg.ch3_wave_pos = self.ch3_wave_pos;
        psg.ch4_lfsr = self.ch4_lfsr;
        psg.ch4_counter = self.ch4_counter;
        psg.ch4_counter_countdown = self.ch4_counter_countdown;
        psg.ch4_alignment = self.ch4_alignment;
        psg.ch4_counter_active = self.ch4_counter_active;
        psg.ch4_background_counter_active = self.ch4_background_counter_active;
        psg.ch4_did_step_counter = self.ch4_did_step_counter;
        psg.ch4_countdown_reloaded = self.ch4_countdown_reloaded;
    }
}

impl Psg {
    pub(in crate::hardware::apu) fn write_state(&self, writer: &mut StateWriter) {
        let start = writer.position();
        PsgSaveState::capture(self).write(writer);
        debug_assert_eq!(writer.position() - start, SAVE_STATE_SIZE);
    }

    pub(in crate::hardware::apu) fn read_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<()> {
        PsgSaveState::read(reader)?.apply(self);
        Ok(())
    }

    pub(in crate::hardware::apu) fn migrate_legacy_state(&mut self, io: &[u8]) {
        debug_assert!(io.len() >= 0xA0);
        let mut state = PsgSaveState::reset();
        state.wave_ram.copy_from_slice(&io[0x90..0xA0]);
        state.apply(self);
        if io[0x84] & 0x80 == 0 {
            return;
        }

        self.write(NR52, 0x80);
        for &(offset, address) in LEGACY_REGISTER_MAP {
            let mut value = io[offset];
            if matches!(address, NR14 | NR24 | NR34 | NR44) {
                value &= !0x80;
            }
            self.write(address, value);
        }
    }
}

fn write_channel(writer: &mut StateWriter, channel: &ChannelState) {
    writer.write_bool(channel.enabled);
    writer.write_bool(channel.length_enabled);
    writer.write_u16(channel.length_counter);
    writer.write_u8(channel.sweep_period);
    writer.write_bool(channel.sweep_negate);
    writer.write_bool(channel.sweep_negate_used);
    writer.write_u8(channel.sweep_shift);
    writer.write_u8(channel.sweep_timer);
    writer.write_u16(channel.sweep_shadow_freq);
    writer.write_bool(channel.sweep_enabled);
    writer.write_u8(channel.envelope_period);
    writer.write_bool(channel.envelope_increase);
    writer.write_u8(channel.envelope_volume);
    writer.write_u8(channel.envelope_timer);
    writer.write_bool(channel.envelope_zero_period_arm);
    writer.write_u8(channel.envelope_forced_tick_delay);
}

fn read_channel(reader: &mut StateReader<'_>) -> Result<ChannelState> {
    Ok(ChannelState {
        enabled: reader.read_bool()?,
        length_enabled: reader.read_bool()?,
        length_counter: reader.read_u16()?,
        sweep_period: reader.read_u8()?,
        sweep_negate: reader.read_bool()?,
        sweep_negate_used: reader.read_bool()?,
        sweep_shift: reader.read_u8()?,
        sweep_timer: reader.read_u8()?,
        sweep_shadow_freq: reader.read_u16()?,
        sweep_enabled: reader.read_bool()?,
        envelope_period: reader.read_u8()?,
        envelope_increase: reader.read_bool()?,
        envelope_volume: reader.read_u8()?,
        envelope_timer: reader.read_u8()?,
        envelope_zero_period_arm: reader.read_bool()?,
        envelope_forced_tick_delay: reader.read_u8()?,
    })
}

const LEGACY_REGISTER_MAP: &[(usize, u16)] = &[
    (0x060, NR10),
    (0x062, NR11),
    (0x063, NR12),
    (0x064, NR13),
    (0x065, NR14),
    (0x068, NR21),
    (0x069, NR22),
    (0x06C, NR23),
    (0x06D, NR24),
    (0x070, NR30),
    (0x071, NR31),
    (0x072, NR32),
    (0x073, NR33),
    (0x074, NR34),
    (0x078, NR41),
    (0x079, NR42),
    (0x07C, NR43),
    (0x07D, NR44),
    (0x080, NR50),
    (0x081, NR51),
];

use std::fmt;

use super::Apu;

mod frame_seq;
mod mixing;
mod noise;
mod runtime;
mod square;
mod wave;

const APU_T_CYCLES_PER_SECOND: f64 = 4_194_304.0;
const APU_INITIAL_SAMPLE_CAPACITY: usize = 2048;
const DEBUG_SAMPLE_HISTORY_LEN: usize = 512;
const DEBUG_CAPTURE_DECIMATION_T_CYCLES: u64 = 64;

const NR10: u16 = 0xFF10;
const NR11: u16 = 0xFF11;
const NR12: u16 = 0xFF12;
const NR13: u16 = 0xFF13;
const NR14: u16 = 0xFF14;
const NR21: u16 = 0xFF16;
const NR22: u16 = 0xFF17;
const NR23: u16 = 0xFF18;
const NR24: u16 = 0xFF19;
const NR30: u16 = 0xFF1A;
const NR31: u16 = 0xFF1B;
const NR32: u16 = 0xFF1C;
const NR33: u16 = 0xFF1D;
const NR34: u16 = 0xFF1E;
const NR41: u16 = 0xFF20;
const NR42: u16 = 0xFF21;
const NR43: u16 = 0xFF22;
const NR44: u16 = 0xFF23;
const NR50: u16 = 0xFF24;
const NR51: u16 = 0xFF25;
const NR52: u16 = 0xFF26;
const WAVE_RAM_START: u16 = 0xFF30;
const WAVE_RAM_END: u16 = 0xFF3F;
const CGB_PCM12: u16 = 0xFF76;
const CGB_PCM34: u16 = 0xFF77;

const FRAME_SEQUENCER_PERIOD_CYCLES: u64 = 8192;

const NR10_READ_MASK: u8 = 0x80;
const NR11_READ_MASK: u8 = 0x3F;
const NR12_READ_MASK: u8 = 0x00;
const NR13_READ_MASK: u8 = 0xFF;
const NR14_READ_MASK: u8 = 0xBF;
const NR15_READ_MASK: u8 = 0xFF;
const NR21_READ_MASK: u8 = 0x3F;
const NR22_READ_MASK: u8 = 0x00;
const NR23_READ_MASK: u8 = 0xFF;
const NR24_READ_MASK: u8 = 0xBF;
const NR30_READ_MASK: u8 = 0x7F;
const NR31_READ_MASK: u8 = 0xFF;
const NR32_READ_MASK: u8 = 0x9F;
const NR33_READ_MASK: u8 = 0xFF;
const NR34_READ_MASK: u8 = 0xBF;
const NR35_READ_MASK: u8 = 0xFF;
const NR41_READ_MASK: u8 = 0xFF;
const NR42_READ_MASK: u8 = 0x00;
const NR43_READ_MASK: u8 = 0x00;
const NR44_READ_MASK: u8 = 0xBF;
const NR50_READ_MASK: u8 = 0x00;
const NR51_READ_MASK: u8 = 0x00;
const NR52_READ_MASK: u8 = 0x70;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PsgChannelSnapshot {
    pub(super) ch1_enabled: bool,
    pub(super) ch1_frequency: u16,
    pub(super) ch1_volume: u8,
    pub(super) ch2_enabled: bool,
    pub(super) ch2_frequency: u16,
    pub(super) ch2_volume: u8,
    pub(super) ch3_enabled: bool,
    pub(super) ch3_frequency: u16,
    pub(super) ch3_output_level: u8,
    pub(super) ch4_enabled: bool,
    pub(super) ch4_volume: u8,
}

#[derive(Clone, Copy, Default)]
struct ChannelState {
    enabled: bool,
    length_enabled: bool,
    length_counter: u16,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_negate_used: bool,
    sweep_shift: u8,
    sweep_timer: u8,
    sweep_shadow_freq: u16,
    sweep_enabled: bool,
    envelope_period: u8,
    envelope_increase: bool,
    envelope_volume: u8,
    envelope_timer: u8,
    envelope_zero_period_arm: bool,
    envelope_forced_tick_delay: u8,
}

#[derive(Clone, Copy)]
struct ChannelDebugSamples {
    samples: [f32; DEBUG_SAMPLE_HISTORY_LEN],
    write_pos: usize,
}

impl Default for ChannelDebugSamples {
    fn default() -> Self {
        Self {
            samples: [0.0; DEBUG_SAMPLE_HISTORY_LEN],
            write_pos: 0,
        }
    }
}

impl ChannelDebugSamples {
    fn push(&mut self, sample: f32) {
        self.samples[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % DEBUG_SAMPLE_HISTORY_LEN;
    }

    fn clear(&mut self) {
        self.samples = [0.0; DEBUG_SAMPLE_HISTORY_LEN];
        self.write_pos = 0;
    }
}

#[derive(Clone)]
pub(super) struct Psg {
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
    sample_rate: u32,
    sample_buffer: Vec<f32>,
    sample_cycle_accum: f64,
    debug_capture_enabled: bool,
    sample_generation_enabled: bool,
    apu_enabled: bool,
    debug_capture_cycle_accum: u64,
    channel_debug_history: [ChannelDebugSamples; 4],
    master_debug_history: ChannelDebugSamples,
    channel_muted: [bool; 4],
}

impl Psg {
    pub(super) fn new(sample_rate: u32) -> Self {
        Self {
            regs: [0; 0x17],
            wave_ram: [0; 0x10],
            nr52: 0,
            channels: [ChannelState::default(); 4],
            frame_seq_cycle_accum: 0,
            frame_seq_step: 0,
            ch1_timer: 0,
            ch2_timer: 0,
            ch3_timer: 0,
            ch4_timer: 0,
            pulse_noise_cycle_accum: 0,
            wave_cycle_accum: 0,
            noise_cycle_accum: 0,
            ch1_output_delay: 0,
            ch2_output_delay: 0,
            ch1_output_suppressed: false,
            ch2_output_suppressed: false,
            ch1_just_reloaded: false,
            ch2_just_reloaded: false,
            ch1_sweep_pending_disable_delay: 0,
            ch1_sweep_trigger_visibility_delay: 0,
            ch3_output_delay: 0,
            ch3_restart_pending: false,
            ch1_current_duty: 0,
            ch2_current_duty: 0,
            ch1_duty_pos: 0,
            ch2_duty_pos: 0,
            ch3_wave_pos: 0,
            ch4_lfsr: 0x7FFF,
            ch4_counter: 0,
            ch4_counter_countdown: 0,
            ch4_alignment: 0,
            ch4_counter_active: false,
            ch4_background_counter_active: false,
            ch4_did_step_counter: false,
            ch4_countdown_reloaded: false,
            sample_rate: sample_rate.max(8_000),
            sample_buffer: Vec::with_capacity(APU_INITIAL_SAMPLE_CAPACITY),
            sample_cycle_accum: 0.0,
            debug_capture_enabled: false,
            sample_generation_enabled: false,
            apu_enabled: true,
            debug_capture_cycle_accum: 0,
            channel_debug_history: [ChannelDebugSamples::default(); 4],
            master_debug_history: ChannelDebugSamples::default(),
            channel_muted: [false; 4],
        }
    }

    pub(super) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(super) fn disable_host_sample_generation(&mut self) {
        self.sample_generation_enabled = false;
    }
}

impl fmt::Debug for Psg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Psg")
            .field("nr52", &format_args!("{:#04X}", self.nr52))
            .field("sample_rate", &self.sample_rate)
            .field("sample_generation_enabled", &self.sample_generation_enabled)
            .field("frame_seq_step", &self.frame_seq_step)
            .field("channel_muted", &self.channel_muted)
            .field("sample_buffer_len", &self.sample_buffer.len())
            .finish_non_exhaustive()
    }
}

impl Apu {
    pub(crate) fn read_psg(&self, addr: u16) -> u8 {
        self.psg.read(addr)
    }

    pub(crate) fn write_psg(&mut self, addr: u16, value: u8) {
        self.psg.write(addr, value);
    }

    pub(super) fn step_psg(&mut self, cycles: u32) {
        let total = self.psg_cycle_accum.saturating_add(cycles);
        let psg_cycles = total / 4;
        self.psg_cycle_accum = total & 3;
        if psg_cycles == 0 {
            return;
        }
        self.psg.step(u64::from(psg_cycles));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn powered_psg() -> Psg {
        let mut psg = Psg::new(48_000);
        psg.write(NR52, 0x80);
        psg
    }

    #[test]
    fn sweep_period_zero_counts_as_8_but_suppresses_calculation_until_nonzero() {
        let mut psg = powered_psg();
        psg.write(NR12, 0xF0);
        psg.write(NR10, 0x01);
        psg.write(NR13, 100);
        psg.write(NR14, 0x80);
        psg.step(8);

        psg.frame_seq_step = 2;
        for _ in 0..8 {
            psg.frame_sequencer_step();
        }
        assert_eq!(psg.ch1_frequency(), 100);

        psg.write(NR10, 0x11);
        for _ in 0..7 {
            psg.frame_sequencer_step();
        }
        assert_eq!(psg.ch1_frequency(), 100);

        psg.frame_sequencer_step();
        assert_eq!(psg.ch1_frequency(), 150);
    }

    #[test]
    fn sweep_shift_zero_checks_overflow_without_writing_frequency() {
        let mut psg = powered_psg();
        psg.write(NR12, 0xF0);
        psg.write(NR10, 0x10);
        psg.write(NR13, 0xFF);
        psg.write(NR14, 0x87);
        psg.step(8);

        psg.frame_seq_step = 2;
        psg.frame_sequencer_step();

        assert_eq!(psg.ch1_frequency(), 0x07FF);
        assert_eq!(psg.nr52 & 0x01, 0x01);

        psg.step(8);
        assert_eq!(psg.nr52 & 0x01, 0x00);
    }

    #[test]
    fn ch1_sweep_clock_inside_trigger_visibility_delay_does_not_use_restart_frequency() {
        let mut psg = powered_psg();
        psg.write(NR12, 0xF0);
        psg.write(NR10, 0x10);
        psg.write(NR13, 0xFF);
        psg.write(NR14, 0x83);
        psg.step(8);

        psg.write(NR14, 0x87);
        psg.step(4);

        psg.frame_seq_step = 2;
        psg.frame_sequencer_step();
        psg.step(16);

        assert_eq!(psg.ch1_frequency(), 0x07FF);
        assert_eq!(psg.nr52 & 0x01, 0x01);
    }

    #[test]
    fn envelope_period_one_enable_schedules_forced_tick() {
        let mut psg = powered_psg();
        psg.write(NR12, 0x08);
        psg.write(NR14, 0x80);

        psg.write(NR12, 0x09);

        assert_eq!(psg.channels[0].envelope_volume, 1);
        assert_eq!(psg.channels[0].envelope_forced_tick_delay, 1);

        psg.frame_seq_cycle_accum = FRAME_SEQUENCER_PERIOD_CYCLES - 1;
        psg.step(1);

        assert_eq!(psg.channels[0].envelope_volume, 2);
        assert_eq!(psg.channels[0].envelope_forced_tick_delay, 0);
    }

    #[test]
    fn square_trigger_output_is_delayed_from_inactive_state() {
        let mut psg = powered_psg();
        psg.write(NR50, 0x77);
        psg.write(NR51, 0x11);
        psg.write(NR12, 0xF0);
        psg.write(NR11, 0xC0);
        psg.write(NR13, 0xFF);
        psg.write(NR14, 0x87);

        assert_eq!(psg.ch1_output_delay, 12);
        assert_eq!(psg.square_sample(0, psg.ch1_duty_pos), 0.0);

        psg.step(8);
        assert_eq!(psg.square_sample(0, psg.ch1_duty_pos), 0.0);

        psg.step(4);
        assert_eq!(psg.ch1_output_delay, 0);
        assert!(psg.square_sample(0, psg.ch1_duty_pos) > 0.0);
    }

    #[test]
    fn noise_frequency_write_clamps_countdown_when_new_divisor_is_shorter() {
        let mut psg = powered_psg();
        psg.regs[(NR43 - NR10) as usize] = 0x09;
        psg.channels[3].enabled = true;
        psg.ch4_counter_countdown = 4;

        psg.write(NR43, 0x38);

        assert_eq!(psg.ch4_counter_countdown, 2);
    }
}

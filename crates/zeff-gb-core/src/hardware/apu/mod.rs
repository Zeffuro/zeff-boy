mod frame_seq;
mod mixing;
mod noise;
mod runtime;
mod square;
mod state;
#[cfg(test)]
mod tests;
mod wave;

use std::fmt;

const APU_T_CYCLES_PER_SECOND: f64 = 4_194_304.0;
const APU_INITIAL_SAMPLE_CAPACITY: usize = 2048;
const DEBUG_SAMPLE_HISTORY_LEN: usize = 512;
const DEBUG_CAPTURE_DECIMATION_T_CYCLES: u64 = 64;
const DIV_APU_SKIP_INACTIVE: u8 = 0;
const DIV_APU_SKIP_NEXT: u8 = 1;
const DIV_APU_SKIP_REPLAY_FIRST_CLOCK: u8 = 2;

#[derive(Clone, Copy, Debug, Default)]
pub struct ApuChannelSnapshot {
    pub ch1_enabled: bool,
    pub ch1_frequency: u16,
    pub ch1_volume: u8,
    pub ch2_enabled: bool,
    pub ch2_frequency: u16,
    pub ch2_volume: u8,
    pub ch3_enabled: bool,
    pub ch3_frequency: u16,
    pub ch3_output_level: u8,
    pub ch4_enabled: bool,
    pub ch4_volume: u8,
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
pub struct ChannelDebugSamples {
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

    fn ordered(&self) -> [f32; DEBUG_SAMPLE_HISTORY_LEN] {
        let mut out = [0.0; DEBUG_SAMPLE_HISTORY_LEN];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.samples[(self.write_pos + i) % DEBUG_SAMPLE_HISTORY_LEN];
        }
        out
    }
}

pub struct Apu {
    regs: [u8; 0x17],
    wave_ram: [u8; 0x10],
    nr52: u8,
    channels: [ChannelState; 4],
    frame_seq_cycle_accum: u64,
    frame_seq_step: u8,
    div_apu_phase_high: bool,
    div_apu_skip_state: u8,
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
    ch3_wave_access_window: u64,
    ch3_wave_access_index: usize,
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
    pub sample_rate: u32,
    sample_buffer: Vec<f32>,
    sample_cycle_accum: f64,
    cgb_hardware: bool,
    cgb_double_speed: bool,
    pub debug_capture_enabled: bool,
    pub sample_generation_enabled: bool,
    pub apu_enabled: bool,
    debug_capture_cycle_accum: u64,
    channel_debug_history: [ChannelDebugSamples; 4],
    master_debug_history: ChannelDebugSamples,
    channel_muted: [bool; 4],
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

impl Apu {
    pub fn new() -> Self {
        Self {
            regs: [0; 0x17],
            wave_ram: [0; 0x10],
            nr52: 0,
            channels: [ChannelState::default(); 4],
            frame_seq_cycle_accum: 0,
            frame_seq_step: 0,
            div_apu_phase_high: false,
            div_apu_skip_state: DIV_APU_SKIP_INACTIVE,
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
            ch3_wave_access_window: 0,
            ch3_wave_access_index: 0,
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
            sample_rate: 48_000,
            sample_buffer: Vec::with_capacity(APU_INITIAL_SAMPLE_CAPACITY),
            sample_cycle_accum: 0.0,
            cgb_hardware: false,
            cgb_double_speed: false,
            debug_capture_enabled: false,
            sample_generation_enabled: true,
            apu_enabled: true,
            debug_capture_cycle_accum: 0,
            channel_debug_history: [ChannelDebugSamples::default(); 4],
            master_debug_history: ChannelDebugSamples::default(),
            channel_muted: [false; 4],
        }
    }
}

impl fmt::Debug for Apu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Apu")
            .field("nr52", &format_args!("{:#04X}", self.nr52))
            .field("sample_rate", &self.sample_rate)
            .field("sample_generation_enabled", &self.sample_generation_enabled)
            .field("debug_capture_enabled", &self.debug_capture_enabled)
            .field("frame_seq_step", &self.frame_seq_step)
            .field("channel_muted", &self.channel_muted)
            .field("sample_buffer_len", &self.sample_buffer.len())
            .finish_non_exhaustive()
    }
}

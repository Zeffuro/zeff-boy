mod noise;
mod runtime;
mod square;
mod state;
#[cfg(test)]
mod tests;
mod wave;

const APU_T_CYCLES_PER_SECOND: f64 = 4_194_304.0;
const APU_INITIAL_SAMPLE_CAPACITY: usize = 2048;
const DEBUG_SAMPLE_HISTORY_LEN: usize = 512;
const DEBUG_CAPTURE_DECIMATION_T_CYCLES: u64 = 64;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ApuChannelSnapshot {
    pub(crate) ch1_enabled: bool,
    pub(crate) ch1_frequency: u16,
    pub(crate) ch1_volume: u8,
    pub(crate) ch2_enabled: bool,
    pub(crate) ch2_frequency: u16,
    pub(crate) ch2_volume: u8,
    pub(crate) ch3_enabled: bool,
    pub(crate) ch3_frequency: u16,
    pub(crate) ch3_output_level: u8,
    pub(crate) ch4_enabled: bool,
    pub(crate) ch4_volume: u8,
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
}

#[derive(Clone, Copy)]
pub(crate) struct ChannelDebugSamples {
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

pub(crate) struct Apu {
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
    ch1_duty_pos: u8,
    ch2_duty_pos: u8,
    ch3_wave_pos: u8,
    ch4_lfsr: u16,
    pub(crate) sample_rate: u32,
    sample_buffer: Vec<f32>,
    sample_cycle_accum: f64,
    pub(crate) debug_capture_enabled: bool,
    pub(crate) sample_generation_enabled: bool,
    debug_capture_cycle_accum: u64,
    channel_debug_history: [ChannelDebugSamples; 4],
    master_debug_history: ChannelDebugSamples,
    channel_muted: [bool; 4],
}

impl Apu {
    pub(crate) fn new() -> Self {
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
            ch1_duty_pos: 0,
            ch2_duty_pos: 0,
            ch3_wave_pos: 0,
            ch4_lfsr: 0x7FFF,
            sample_rate: 48_000,
            sample_buffer: Vec::with_capacity(APU_INITIAL_SAMPLE_CAPACITY),
            sample_cycle_accum: 0.0,
            debug_capture_enabled: false,
            sample_generation_enabled: true,
            debug_capture_cycle_accum: 0,
            channel_debug_history: [ChannelDebugSamples::default(); 4],
            master_debug_history: ChannelDebugSamples::default(),
            channel_muted: [false; 4],
        }
    }
}

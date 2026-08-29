mod dmc;
mod filter;
mod mixing;
mod noise;
mod pulse;
mod runtime;
mod triangle;

use crate::hardware::timing::NesTiming;
use filter::NesOutputFilter;
use std::collections::VecDeque;
use std::fmt;

const INITIAL_SAMPLE_CAPACITY: usize = 2048;
const DEBUG_SAMPLE_CAPACITY: usize = 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct ApuChannelSnapshot {
    pub pulse1_enabled: bool,
    pub pulse1_timer_period: u16,
    pub pulse1_volume: u8,
    pub pulse2_enabled: bool,
    pub pulse2_timer_period: u16,
    pub pulse2_volume: u8,
    pub triangle_enabled: bool,
    pub triangle_timer_period: u16,
    pub triangle_volume: u8,
    pub noise_enabled: bool,
    pub noise_volume: u8,
    pub dmc_enabled: bool,
    pub dmc_output_level: u8,
}

#[derive(Clone)]
pub struct Apu {
    pub pulse1: pulse::Pulse,
    pub pulse2: pulse::Pulse,
    pub triangle: triangle::Triangle,
    pub noise: noise::Noise,
    pub dmc: dmc::Dmc,

    pub five_step_mode: bool,
    pub irq_inhibit: bool,
    pub frame_irq: bool,
    pub frame_cycle: u64,
    pub frame_reset_delay: u8,
    pending_frame_counter_value: Option<u8>,
    frame_clock_block: u8,
    clock_half_rate_timers: bool,
    timing: NesTiming,

    pub sample_buffer: Vec<f32>,
    pub output_sample_rate: f64,
    cpu_clock_hz: f64,
    sample_accumulator: f64,
    sample_generation_enabled: bool,
    output_filter: NesOutputFilter,
    debug_collection_enabled: bool,
    channel_mutes: [bool; 5],

    pub expansion_audio: f32,
    master_debug_samples: VecDeque<f32>,
    pulse1_debug_samples: VecDeque<f32>,
    pulse2_debug_samples: VecDeque<f32>,
    triangle_debug_samples: VecDeque<f32>,
    noise_debug_samples: VecDeque<f32>,
}

impl Apu {
    pub fn new(output_sample_rate: f64) -> Self {
        Self::new_with_timing(output_sample_rate, NesTiming::Ntsc)
    }

    pub(crate) fn new_with_timing(output_sample_rate: f64, timing: NesTiming) -> Self {
        let (cpu_clock_hz_numerator, cpu_clock_hz_denominator) = timing.cpu_clock_hz_ratio();
        Self {
            pulse1: pulse::Pulse::new(true),
            pulse2: pulse::Pulse::new(false),
            triangle: triangle::Triangle::new(),
            noise: noise::Noise::new_with_timing(timing),
            dmc: dmc::Dmc::new_with_timing(timing),
            five_step_mode: false,
            irq_inhibit: false,
            frame_irq: false,
            frame_cycle: 9,
            frame_reset_delay: 0,
            pending_frame_counter_value: None,
            frame_clock_block: 0,
            clock_half_rate_timers: false,
            timing,
            sample_buffer: Vec::with_capacity(INITIAL_SAMPLE_CAPACITY),
            output_sample_rate,
            cpu_clock_hz: cpu_clock_hz_numerator as f64 / cpu_clock_hz_denominator as f64,
            sample_accumulator: 0.0,
            sample_generation_enabled: true,
            output_filter: NesOutputFilter::new(output_sample_rate),
            debug_collection_enabled: true,
            channel_mutes: [false; 5],
            expansion_audio: 0.0,
            master_debug_samples: VecDeque::with_capacity(DEBUG_SAMPLE_CAPACITY),
            pulse1_debug_samples: VecDeque::with_capacity(DEBUG_SAMPLE_CAPACITY),
            pulse2_debug_samples: VecDeque::with_capacity(DEBUG_SAMPLE_CAPACITY),
            triangle_debug_samples: VecDeque::with_capacity(DEBUG_SAMPLE_CAPACITY),
            noise_debug_samples: VecDeque::with_capacity(DEBUG_SAMPLE_CAPACITY),
        }
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        self.pulse1.write_state(w);
        self.pulse2.write_state(w);
        self.triangle.write_state(w);
        self.noise.write_state(w);
        self.dmc.write_state(w);
        w.write_bool(self.five_step_mode);
        w.write_bool(self.irq_inhibit);
        w.write_bool(self.frame_irq);
        w.write_u64(self.frame_cycle);
        w.write_f64(crate::emulator::DEFAULT_SAMPLE_RATE);
        w.write_f64(0.0);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        let runtime_output_sample_rate = self.output_sample_rate;

        self.pulse1.read_state(r)?;
        self.pulse2.read_state(r)?;
        self.triangle.read_state(r)?;
        self.noise.read_state(r)?;
        self.dmc.read_state(r)?;
        self.five_step_mode = r.read_bool()?;
        self.irq_inhibit = r.read_bool()?;
        self.frame_irq = r.read_bool()?;
        self.frame_cycle = r.read_u64()?;
        self.frame_reset_delay = 0;
        self.pending_frame_counter_value = None;
        self.frame_clock_block = 0;
        self.clock_half_rate_timers = self.frame_cycle.is_multiple_of(2);
        let _saved_output_sample_rate = r.read_f64()?;
        let _saved_sample_accumulator = r.read_f64()?;
        self.output_sample_rate = runtime_output_sample_rate;
        self.sample_accumulator = 0.0;
        self.output_filter = NesOutputFilter::new(runtime_output_sample_rate);

        self.sample_buffer.clear();
        self.master_debug_samples.clear();
        self.pulse1_debug_samples.clear();
        self.pulse2_debug_samples.clear();
        self.triangle_debug_samples.clear();
        self.noise_debug_samples.clear();
        Ok(())
    }

    pub(crate) fn write_frame_counter_runtime_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.frame_reset_delay);
        w.write_bool(self.pending_frame_counter_value.is_some());
        w.write_u8(self.pending_frame_counter_value.unwrap_or(0));
        w.write_u8(self.frame_clock_block);
        w.write_bool(self.clock_half_rate_timers);
    }

    pub(crate) fn read_frame_counter_runtime_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        let frame_reset_delay = r.read_u8()?;
        let has_pending_value = r.read_bool()?;
        let pending_value = r.read_u8()?;
        let frame_clock_block = r.read_u8()?;
        let clock_half_rate_timers = r.read_bool()?;

        if frame_reset_delay > 4 {
            anyhow::bail!("invalid APU frame reset delay in save-state: {frame_reset_delay}");
        }
        if has_pending_value != (frame_reset_delay != 0) {
            anyhow::bail!("inconsistent pending APU frame-counter write in save-state");
        }
        if frame_clock_block > 2 {
            anyhow::bail!("invalid APU frame clock block in save-state: {frame_clock_block}");
        }

        self.frame_reset_delay = frame_reset_delay;
        self.pending_frame_counter_value = has_pending_value.then_some(pending_value);
        self.frame_clock_block = frame_clock_block;
        self.clock_half_rate_timers = clock_half_rate_timers;
        Ok(())
    }
}

impl fmt::Debug for Apu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("APU")
            .field("five_step_mode", &self.five_step_mode)
            .field("frame_irq", &self.frame_irq)
            .field("frame_cycle", &self.frame_cycle)
            .field("frame_reset_delay", &self.frame_reset_delay)
            .field(
                "pending_frame_counter_value",
                &self.pending_frame_counter_value,
            )
            .field("frame_clock_block", &self.frame_clock_block)
            .field("buffered_samples", &self.sample_buffer.len())
            .finish_non_exhaustive()
    }
}

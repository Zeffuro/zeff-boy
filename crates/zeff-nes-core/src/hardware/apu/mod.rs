mod dmc;
mod mixing;
mod noise;
mod pulse;
mod runtime;
mod triangle;

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
}

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

    pub sample_buffer: Vec<f32>,
    pub output_sample_rate: f64,
    sample_accumulator: f64,
    sample_generation_enabled: bool,
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
        Self {
            pulse1: pulse::Pulse::new(true),
            pulse2: pulse::Pulse::new(false),
            triangle: triangle::Triangle::new(),
            noise: noise::Noise::new(),
            dmc: dmc::Dmc::new(),
            five_step_mode: false,
            irq_inhibit: false,
            frame_irq: false,
            frame_cycle: 0,
            sample_buffer: Vec::with_capacity(INITIAL_SAMPLE_CAPACITY),
            output_sample_rate,
            sample_accumulator: 0.0,
            sample_generation_enabled: true,
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
        w.write_f64(self.output_sample_rate);
        w.write_f64(self.sample_accumulator);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.pulse1.read_state(r)?;
        self.pulse2.read_state(r)?;
        self.triangle.read_state(r)?;
        self.noise.read_state(r)?;
        self.dmc.read_state(r)?;
        self.five_step_mode = r.read_bool()?;
        self.irq_inhibit = r.read_bool()?;
        self.frame_irq = r.read_bool()?;
        self.frame_cycle = r.read_u64()?;
        self.output_sample_rate = r.read_f64()?;
        self.sample_accumulator = r.read_f64()?;

        self.sample_buffer.clear();
        self.master_debug_samples.clear();
        self.pulse1_debug_samples.clear();
        self.pulse2_debug_samples.clear();
        self.triangle_debug_samples.clear();
        self.noise_debug_samples.clear();
        Ok(())
    }
}

impl fmt::Debug for Apu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("APU")
            .field("five_step_mode", &self.five_step_mode)
            .field("frame_irq", &self.frame_irq)
            .field("frame_cycle", &self.frame_cycle)
            .field("buffered_samples", &self.sample_buffer.len())
            .finish_non_exhaustive()
    }
}

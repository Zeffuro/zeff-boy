mod dmc;
mod noise;
mod pulse;
mod triangle;

use std::collections::VecDeque;
use std::fmt;

use crate::hardware::constants::*;

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
    channel_mutes: [bool; 4],
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
            channel_mutes: [false; 4],
            master_debug_samples: VecDeque::with_capacity(DEBUG_SAMPLE_CAPACITY),
            pulse1_debug_samples: VecDeque::with_capacity(DEBUG_SAMPLE_CAPACITY),
            pulse2_debug_samples: VecDeque::with_capacity(DEBUG_SAMPLE_CAPACITY),
            triangle_debug_samples: VecDeque::with_capacity(DEBUG_SAMPLE_CAPACITY),
            noise_debug_samples: VecDeque::with_capacity(DEBUG_SAMPLE_CAPACITY),
        }
    }

    pub fn write_register(&mut self, addr: u16, val: u8, odd_cycle: bool) {
        match addr {
            0x4000..=0x4003 => self.pulse1.write(addr - 0x4000, val),
            0x4004..=0x4007 => self.pulse2.write(addr - 0x4004, val),
            0x4008..=0x400B => self.triangle.write(addr - 0x4008, val),
            0x400C..=0x400F => self.noise.write(addr - 0x400C, val),
            0x4010..=0x4013 => self.dmc.write(addr - 0x4010, val),
            0x4015 => {
                self.pulse1.set_enabled(val & 0x01 != 0);
                self.pulse2.set_enabled(val & 0x02 != 0);
                self.triangle.set_enabled(val & 0x04 != 0);
                self.noise.set_enabled(val & 0x08 != 0);
                self.dmc.set_enabled(val & 0x10 != 0);
                self.frame_irq = false;
            }
            0x4017 => {
                let _old_mode = self.five_step_mode;
                self.five_step_mode = val & 0x80 != 0;
                self.irq_inhibit = val & 0x40 != 0;
                if self.irq_inhibit {
                    self.frame_irq = false;
                }

                self.frame_cycle = if self.five_step_mode || !odd_cycle { 0 } else { 1 };

                if self.five_step_mode {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }
            }
            _ => {}
        }
    }

    pub fn read_status(&mut self) -> u8 {
        let mut status = 0u8;
        if self.pulse1.length_counter > 0 { status |= 0x01; }
        if self.pulse2.length_counter > 0 { status |= 0x02; }
        if self.triangle.length_counter > 0 { status |= 0x04; }
        if self.noise.length_counter > 0 { status |= 0x08; }
        if self.dmc.bytes_remaining > 0 { status |= 0x10; }
        if self.frame_irq { status |= 0x40; }
        if self.dmc.irq_flag { status |= 0x80; }
        self.frame_irq = false;
        status
    }
    pub fn tick(&mut self) {
        self.triangle.tick();
        self.dmc.tick();

        if self.frame_cycle % 2 == 0 {
            self.pulse1.tick();
            self.pulse2.tick();
            self.noise.tick();
        }

        self.step_frame_counter();
        self.generate_sample();
        self.frame_cycle += 1;
    }

    pub fn irq_pending(&self) -> bool {
        self.frame_irq || self.dmc.irq_flag
    }

    fn step_frame_counter(&mut self) {
        if !self.five_step_mode {
            match self.frame_cycle {
                FRAME_STEP_1 | FRAME_STEP_3 => self.clock_quarter_frame(),
                FRAME_STEP_2 => {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }
                FRAME_STEP_4 => {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                    if !self.irq_inhibit {
                        self.frame_irq = true;
                    }
                    self.frame_cycle = 0;
                    return;
                }
                _ => {}
            }
        } else {
            match self.frame_cycle {
                FRAME_STEP_1 | FRAME_STEP_3 => self.clock_quarter_frame(),
                FRAME_STEP_2 => {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }
                FRAME_STEP_5 => {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                    self.frame_cycle = 0;
                    return;
                }
                _ => {}
            }
        }
    }

    fn clock_quarter_frame(&mut self) {
        self.pulse1.clock_envelope();
        self.pulse2.clock_envelope();
        self.triangle.clock_linear_counter();
        self.noise.clock_envelope();
    }

    fn clock_half_frame(&mut self) {
        self.pulse1.clock_length();
        self.pulse2.clock_length();
        self.triangle.clock_length();
        self.noise.clock_length();
        self.pulse1.clock_sweep();
        self.pulse2.clock_sweep();
    }

    fn generate_sample(&mut self) {
        let p1_raw = self.pulse1.output() as f32;
        let p2_raw = self.pulse2.output() as f32;
        let tri_raw = self.triangle.output() as f32;
        let noi_raw = self.noise.output() as f32;
        let _dmc = self.dmc.output() as f32;

        let p1 = if self.channel_mutes[0] { 0.0 } else { p1_raw };
        let p2 = if self.channel_mutes[1] { 0.0 } else { p2_raw };
        let tri = if self.channel_mutes[2] { 0.0 } else { tri_raw };
        let noi = if self.channel_mutes[3] { 0.0 } else { noi_raw };

        let pulse_out = MIX_PULSE * (p1 + p2);
        let tnd_out = MIX_TND_TRI * tri + MIX_TND_NOISE * noi + MIX_TND_DMC * _dmc;
        let sample = pulse_out + tnd_out;

        self.sample_accumulator += self.output_sample_rate;
        if self.sample_accumulator >= APU_CPU_CLOCK_NTSC {
            self.sample_accumulator -= APU_CPU_CLOCK_NTSC;
            self.sample_buffer.push(sample);

            Self::push_debug_sample(&mut self.master_debug_samples, sample.clamp(-1.0, 1.0));
            Self::push_debug_sample(&mut self.pulse1_debug_samples, (p1_raw / 15.0).clamp(-1.0, 1.0));
            Self::push_debug_sample(&mut self.pulse2_debug_samples, (p2_raw / 15.0).clamp(-1.0, 1.0));
            Self::push_debug_sample(&mut self.triangle_debug_samples, ((tri_raw - 7.5) / 7.5).clamp(-1.0, 1.0));
            Self::push_debug_sample(&mut self.noise_debug_samples, (noi_raw / 15.0).clamp(-1.0, 1.0));
        }
    }

    fn push_debug_sample(samples: &mut VecDeque<f32>, sample: f32) {
        if samples.len() >= DEBUG_SAMPLE_CAPACITY {
            let _ = samples.pop_front();
        }
        samples.push_back(sample);
    }

    pub fn drain_samples(&mut self) -> Vec<f32> {
        std::mem::replace(&mut self.sample_buffer, Vec::with_capacity(INITIAL_SAMPLE_CAPACITY))
    }

    pub fn channel_snapshot(&self) -> ApuChannelSnapshot {
        ApuChannelSnapshot {
            pulse1_enabled: self.pulse1.midi_active(),
            pulse1_timer_period: self.pulse1.timer_period(),
            pulse1_volume: self.pulse1.midi_volume(),
            pulse2_enabled: self.pulse2.midi_active(),
            pulse2_timer_period: self.pulse2.timer_period(),
            pulse2_volume: self.pulse2.midi_volume(),
            triangle_enabled: self.triangle.midi_active(),
            triangle_timer_period: self.triangle.timer_period(),
            triangle_volume: self.triangle.midi_volume(),
            noise_enabled: self.noise.midi_active(),
            noise_volume: self.noise.midi_volume(),
        }
    }

    pub fn set_channel_mutes(&mut self, mutes: [bool; 4]) {
        self.channel_mutes = mutes;
    }

    pub fn channel_mutes(&self) -> [bool; 4] {
        self.channel_mutes
    }

    pub fn master_debug_samples_ordered(&self) -> Vec<f32> {
        self.master_debug_samples.iter().copied().collect()
    }

    pub fn channel_debug_samples_ordered(&self, channel: usize) -> Vec<f32> {
        match channel {
            0 => self.pulse1_debug_samples.iter().copied().collect(),
            1 => self.pulse2_debug_samples.iter().copied().collect(),
            2 => self.triangle_debug_samples.iter().copied().collect(),
            3 => self.noise_debug_samples.iter().copied().collect(),
            _ => Vec::new(),
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


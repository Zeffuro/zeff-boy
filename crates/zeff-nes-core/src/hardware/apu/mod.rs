mod dmc;
mod noise;
mod pulse;
mod triangle;

use std::fmt;

use crate::hardware::constants::*;

const INITIAL_SAMPLE_CAPACITY: usize = 2048;

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
        }
    }

    pub fn write_register(&mut self, addr: u16, val: u8) {
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
                self.five_step_mode = val & 0x80 != 0;
                self.irq_inhibit = val & 0x40 != 0;
                if self.irq_inhibit {
                    self.frame_irq = false;
                }
                self.frame_cycle = 0;
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
        if self.five_step_mode {
            match self.frame_cycle {
                FRAME_QUARTER_1 => self.clock_quarter_frame(),
                FRAME_QUARTER_2 => { self.clock_quarter_frame(); self.clock_half_frame(); }
                FRAME_QUARTER_3 => self.clock_quarter_frame(),
                FRAME_5STEP_END => { self.clock_quarter_frame(); self.clock_half_frame(); self.frame_cycle = 0; }
                _ => {}
            }
        } else {
            match self.frame_cycle {
                FRAME_QUARTER_1 => self.clock_quarter_frame(),
                FRAME_QUARTER_2 => { self.clock_quarter_frame(); self.clock_half_frame(); }
                FRAME_QUARTER_3 => self.clock_quarter_frame(),
                FRAME_4STEP_END => {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                    if !self.irq_inhibit { self.frame_irq = true; }
                    self.frame_cycle = 0;
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
        let _p1 = self.pulse1.output() as f32;
        let _p2 = self.pulse2.output() as f32;
        let _tri = self.triangle.output() as f32;
        let _noi = self.noise.output() as f32;
        let _dmc = self.dmc.output() as f32;

        let pulse_out = MIX_PULSE * (_p1 + _p2);
        let tnd_out = MIX_TND_TRI * _tri + MIX_TND_NOISE * _noi + MIX_TND_DMC * _dmc;
        let sample = pulse_out + tnd_out;

        self.sample_accumulator += self.output_sample_rate;
        if self.sample_accumulator >= APU_CPU_CLOCK_NTSC {
            self.sample_accumulator -= APU_CPU_CLOCK_NTSC;
            self.sample_buffer.push(sample);
        }
    }

    pub fn drain_samples(&mut self) -> Vec<f32> {
        std::mem::replace(&mut self.sample_buffer, Vec::with_capacity(INITIAL_SAMPLE_CAPACITY))
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


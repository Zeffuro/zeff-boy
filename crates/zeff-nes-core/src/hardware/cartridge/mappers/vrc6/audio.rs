use crate::save_state::{StateReader, StateWriter};

/// VRC6 expansion audio: two pulse channels + one sawtooth channel.
pub struct Vrc6Audio {
    pub pulse1: Vrc6Pulse,
    pub pulse2: Vrc6Pulse,
    pub sawtooth: Vrc6Sawtooth,
    halt: bool,
    freq_shift: u8, // 0, 4, or 8
}

impl Vrc6Audio {
    pub fn new() -> Self {
        Self {
            pulse1: Vrc6Pulse::new(),
            pulse2: Vrc6Pulse::new(),
            sawtooth: Vrc6Sawtooth::new(),
            halt: false,
            freq_shift: 0,
        }
    }

    /// Write to $9003 — frequency control for all audio channels.
    pub fn write_freq_control(&mut self, val: u8) {
        self.halt = val & 0x04 != 0;
        self.freq_shift = if val & 0x02 != 0 {
            8
        } else if val & 0x01 != 0 {
            4
        } else {
            0
        };
    }

    /// Tick all audio channels once per CPU cycle.
    pub fn tick(&mut self) {
        if self.halt {
            return;
        }
        self.pulse1.tick(self.freq_shift);
        self.pulse2.tick(self.freq_shift);
        self.sawtooth.tick(self.freq_shift);
    }

    /// Combined expansion audio output, scaled to match NES APU levels.
    pub fn output(&self) -> f32 {
        let p1 = self.pulse1.output() as f32;
        let p2 = self.pulse2.output() as f32;
        let saw = self.sawtooth.output() as f32;
        // Max combined: 15 + 15 + 31 = 61
        // Scale to approximately match the internal APU pulse output range (~0.0 – 0.5)
        (p1 + p2 + saw) / 61.0 * 0.5
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_bool(self.halt);
        w.write_u8(self.freq_shift);
        self.pulse1.write_state(w);
        self.pulse2.write_state(w);
        self.sawtooth.write_state(w);
    }

    pub fn read_state(&mut self, r: &mut StateReader) -> anyhow::Result<()> {
        self.halt = r.read_bool()?;
        self.freq_shift = r.read_u8()?;
        self.pulse1.read_state(r)?;
        self.pulse2.read_state(r)?;
        self.sawtooth.read_state(r)
    }
}

// ─── VRC6 Pulse channel ────────────────────────────────────────────

pub struct Vrc6Pulse {
    mode: bool,  // Direct mode (ignore duty, always output volume)
    duty: u8,    // 0–7
    volume: u8,  // 0–15
    period: u16, // 12-bit
    enabled: bool,
    timer: u16,
    step: u8, // 0–15
}

impl Vrc6Pulse {
    pub fn new() -> Self {
        Self {
            mode: false,
            duty: 0,
            volume: 0,
            period: 0,
            enabled: false,
            timer: 0,
            step: 0,
        }
    }

    /// $x000: MDDDVVVV
    pub fn write_control(&mut self, val: u8) {
        self.mode = val & 0x80 != 0;
        self.duty = (val >> 4) & 0x07;
        self.volume = val & 0x0F;
    }

    /// $x001: LLLLLLLL (period low 8 bits)
    pub fn write_period_low(&mut self, val: u8) {
        self.period = (self.period & 0xF00) | val as u16;
    }

    /// $x002: E...HHHH (enable + period high 4 bits)
    pub fn write_period_high(&mut self, val: u8) {
        self.period = (self.period & 0x0FF) | ((val as u16 & 0x0F) << 8);
        self.enabled = val & 0x80 != 0;
    }

    pub fn tick(&mut self, freq_shift: u8) {
        if !self.enabled {
            return;
        }
        if self.timer == 0 {
            let effective_period = self.period >> freq_shift;
            self.timer = effective_period;
            self.step = (self.step + 1) & 0x0F;
        } else {
            self.timer -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        if self.mode || self.step <= self.duty {
            self.volume
        } else {
            0
        }
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_bool(self.mode);
        w.write_u8(self.duty);
        w.write_u8(self.volume);
        w.write_u16(self.period);
        w.write_bool(self.enabled);
        w.write_u16(self.timer);
        w.write_u8(self.step);
    }

    pub fn read_state(&mut self, r: &mut StateReader) -> anyhow::Result<()> {
        self.mode = r.read_bool()?;
        self.duty = r.read_u8()?;
        self.volume = r.read_u8()?;
        self.period = r.read_u16()?;
        self.enabled = r.read_bool()?;
        self.timer = r.read_u16()?;
        self.step = r.read_u8()?;
        Ok(())
    }
}

// ─── VRC6 Sawtooth channel ─────────────────────────────────────────

pub struct Vrc6Sawtooth {
    rate: u8,
    period: u16,
    enabled: bool,
    timer: u16,
    accumulator: u8,
    step: u8,
}

impl Vrc6Sawtooth {
    pub fn new() -> Self {
        Self {
            rate: 0,
            period: 0,
            enabled: false,
            timer: 0,
            accumulator: 0,
            step: 0,
        }
    }

    pub fn write_rate(&mut self, val: u8) {
        self.rate = val & 0x3F;
    }

    pub fn write_period_low(&mut self, val: u8) {
        self.period = (self.period & 0xF00) | val as u16;
    }

    pub fn write_period_high(&mut self, val: u8) {
        self.period = (self.period & 0x0FF) | ((val as u16 & 0x0F) << 8);
        self.enabled = val & 0x80 != 0;
        if !self.enabled {
            self.accumulator = 0;
            self.step = 0;
        }
    }

    pub fn tick(&mut self, freq_shift: u8) {
        if !self.enabled {
            return;
        }
        if self.timer == 0 {
            let effective_period = self.period >> freq_shift;
            self.timer = effective_period;

            self.step += 1;
            if self.step >= 14 {
                self.step = 0;
                self.accumulator = 0;
            } else if self.step & 1 == 0 {
                self.accumulator = self.accumulator.wrapping_add(self.rate);
            }
        } else {
            self.timer -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        self.accumulator >> 3
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_u8(self.rate);
        w.write_u16(self.period);
        w.write_bool(self.enabled);
        w.write_u16(self.timer);
        w.write_u8(self.accumulator);
        w.write_u8(self.step);
    }

    pub fn read_state(&mut self, r: &mut StateReader) -> anyhow::Result<()> {
        self.rate = r.read_u8()?;
        self.period = r.read_u16()?;
        self.enabled = r.read_bool()?;
        self.timer = r.read_u16()?;
        self.accumulator = r.read_u8()?;
        self.step = r.read_u8()?;
        Ok(())
    }
}

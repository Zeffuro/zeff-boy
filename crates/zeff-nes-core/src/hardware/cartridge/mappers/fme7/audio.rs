const AY_PRESCALER_PERIOD: u16 = 16;

const AY_VOLUME_TABLE: [f32; 16] = [
    0.0, 0.0074, 0.0108, 0.0158, 0.0230, 0.0337, 0.0492, 0.0719, 0.1050, 0.1534, 0.2241, 0.3273,
    0.4781, 0.6984, 1.0, 1.0,
];

pub(super) const SN5B_AUDIO_MARKER: &[u8; 4] = b"SN5B";

pub(super) struct Sunsoft5BAudio {
    command: u8,
    regs: [u8; 16],
    channels: [Sunsoft5BChannel; 3],
    envelope: Sunsoft5BEnvelope,
    prescaler: u16,
}

impl Sunsoft5BAudio {
    pub(super) fn new() -> Self {
        Self {
            command: 0,
            regs: [0; 16],
            channels: [
                Sunsoft5BChannel::new(),
                Sunsoft5BChannel::new(),
                Sunsoft5BChannel::new(),
            ],
            envelope: Sunsoft5BEnvelope::new(),
            prescaler: 0,
        }
    }

    pub(super) fn write_command(&mut self, val: u8) {
        self.command = val & 0x0F;
    }

    pub(super) fn write_data(&mut self, val: u8) {
        let reg = self.command as usize;
        if reg < 16 {
            self.regs[reg] = val;
        }
        match reg {
            0 => self.channels[0].period = (self.channels[0].period & 0xF00) | val as u16,
            1 => {
                self.channels[0].period =
                    (self.channels[0].period & 0x0FF) | ((val as u16 & 0x0F) << 8);
            }
            2 => self.channels[1].period = (self.channels[1].period & 0xF00) | val as u16,
            3 => {
                self.channels[1].period =
                    (self.channels[1].period & 0x0FF) | ((val as u16 & 0x0F) << 8);
            }
            4 => self.channels[2].period = (self.channels[2].period & 0xF00) | val as u16,
            5 => {
                self.channels[2].period =
                    (self.channels[2].period & 0x0FF) | ((val as u16 & 0x0F) << 8);
            }
            7 => {
                self.channels[0].tone_disable = val & 0x01 != 0;
                self.channels[1].tone_disable = val & 0x02 != 0;
                self.channels[2].tone_disable = val & 0x04 != 0;
            }
            8 => {
                self.channels[0].use_envelope = val & 0x10 != 0;
                self.channels[0].volume = val & 0x0F;
            }
            9 => {
                self.channels[1].use_envelope = val & 0x10 != 0;
                self.channels[1].volume = val & 0x0F;
            }
            10 => {
                self.channels[2].use_envelope = val & 0x10 != 0;
                self.channels[2].volume = val & 0x0F;
            }
            11 => {
                self.envelope.period = (self.envelope.period & 0xFF00) | val as u16;
            }
            12 => {
                self.envelope.period = (self.envelope.period & 0x00FF) | ((val as u16) << 8);
            }
            13 => {
                self.envelope.trigger(val & 0x0F);
            }
            _ => {}
        }
    }

    pub(super) fn tick(&mut self) {
        self.prescaler += 1;
        if self.prescaler < AY_PRESCALER_PERIOD {
            return;
        }
        self.prescaler = 0;

        for ch in &mut self.channels {
            ch.tick();
        }
        self.envelope.tick();
    }

    pub(super) fn output(&self) -> f32 {
        let env_vol = self.envelope.output();
        let mut sum = 0.0f32;
        for ch in &self.channels {
            let vol_index = if ch.use_envelope { env_vol } else { ch.volume } as usize;
            let level = AY_VOLUME_TABLE[vol_index.min(15)];
            if ch.tone_disable || ch.output_high {
                sum += level;
            }
        }
        sum * 0.167
    }

    pub(super) fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.command);
        w.write_bytes(&self.regs);
        w.write_u16(self.prescaler);
        for ch in &self.channels {
            ch.write_state(w);
        }
        self.envelope.write_state(w);
    }

    pub(super) fn read_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        self.command = r.read_u8()?;
        r.read_exact(&mut self.regs)?;
        self.prescaler = r.read_u16()?;
        for ch in &mut self.channels {
            ch.read_state(r)?;
        }
        self.envelope.read_state(r)
    }
}

#[derive(Clone)]
struct Sunsoft5BChannel {
    period: u16,
    timer: u16,
    output_high: bool,
    tone_disable: bool,
    volume: u8,
    use_envelope: bool,
}

impl Sunsoft5BChannel {
    fn new() -> Self {
        Self {
            period: 0,
            timer: 0,
            output_high: false,
            tone_disable: false,
            volume: 0,
            use_envelope: false,
        }
    }

    fn tick(&mut self) {
        if self.timer == 0 {
            self.timer = self.period.max(1);
            self.output_high = !self.output_high;
        } else {
            self.timer -= 1;
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u16(self.period);
        w.write_u16(self.timer);
        w.write_bool(self.output_high);
        w.write_bool(self.tone_disable);
        w.write_u8(self.volume);
        w.write_bool(self.use_envelope);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.period = r.read_u16()?;
        self.timer = r.read_u16()?;
        self.output_high = r.read_bool()?;
        self.tone_disable = r.read_bool()?;
        self.volume = r.read_u8()?;
        self.use_envelope = r.read_bool()?;
        Ok(())
    }
}

struct Sunsoft5BEnvelope {
    period: u16,
    timer: u16,
    step: u8,
    shape: u8,
    holding: bool,
    output: u8,
    attack: bool,
    alternate: bool,
    hold: bool,
    cont: bool,
}

impl Sunsoft5BEnvelope {
    fn new() -> Self {
        Self {
            period: 0,
            timer: 0,
            step: 0,
            shape: 0,
            holding: false,
            output: 0,
            attack: false,
            alternate: false,
            hold: false,
            cont: false,
        }
    }

    fn trigger(&mut self, shape: u8) {
        self.shape = shape;
        self.cont = shape & 0x08 != 0;
        self.attack = shape & 0x04 != 0;
        self.alternate = shape & 0x02 != 0;
        self.hold = shape & 0x01 != 0;
        self.step = 0;
        self.holding = false;
        self.update_output();
    }

    fn tick(&mut self) {
        if self.holding {
            return;
        }
        if self.timer == 0 {
            self.timer = self.period.max(1);
            self.step += 1;
            if self.step > 15 {
                if !self.cont {
                    self.output = 0;
                    self.holding = true;
                    return;
                }
                if self.hold {
                    self.holding = true;
                    if self.alternate {
                        self.output = if self.attack { 0 } else { 15 };
                    }
                    return;
                }
                if self.alternate {
                    self.attack = !self.attack;
                }
                self.step = 0;
            }
            self.update_output();
        } else {
            self.timer -= 1;
        }
    }

    fn update_output(&mut self) {
        self.output = if self.attack {
            self.step
        } else {
            15 - self.step
        };
    }

    fn output(&self) -> u8 {
        self.output
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u16(self.period);
        w.write_u16(self.timer);
        w.write_u8(self.step);
        w.write_u8(self.shape);
        w.write_bool(self.holding);
        w.write_u8(self.output);
        w.write_bool(self.attack);
        w.write_bool(self.alternate);
        w.write_bool(self.hold);
        w.write_bool(self.cont);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.period = r.read_u16()?;
        self.timer = r.read_u16()?;
        self.step = r.read_u8()?;
        self.shape = r.read_u8()?;
        self.holding = r.read_bool()?;
        self.output = r.read_u8()?;
        self.attack = r.read_bool()?;
        self.alternate = r.read_bool()?;
        self.hold = r.read_bool()?;
        self.cont = r.read_bool()?;
        Ok(())
    }
}

pub struct Pulse {
    is_pulse1: bool,

    pub enabled: bool,
    pub length_counter: u8,

    duty: u8,
    length_halt: bool,
    constant_volume: bool,
    envelope_volume: u8,

    sweep_enabled: bool,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_reload: bool,
    sweep_divider: u8,

    timer_period: u16,
    timer_counter: u16,

    sequence_pos: u8,

    envelope_start: bool,
    envelope_divider: u8,
    envelope_decay: u8,
}

static DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];

impl Pulse {
    pub fn new(is_pulse1: bool) -> Self {
        Self {
            is_pulse1,
            enabled: false,
            length_counter: 0,
            duty: 0,
            length_halt: false,
            constant_volume: false,
            envelope_volume: 0,
            sweep_enabled: false,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_reload: false,
            sweep_divider: 0,
            timer_period: 0,
            timer_counter: 0,
            sequence_pos: 0,
            envelope_start: false,
            envelope_divider: 0,
            envelope_decay: 0,
        }
    }

    pub fn write(&mut self, offset: u16, val: u8) {
        match offset {
            0 => {
                self.duty = (val >> 6) & 0x03;
                self.length_halt = val & 0x20 != 0;
                self.constant_volume = val & 0x10 != 0;
                self.envelope_volume = val & 0x0F;
            }
            1 => {
                self.sweep_enabled = val & 0x80 != 0;
                self.sweep_period = (val >> 4) & 0x07;
                self.sweep_negate = val & 0x08 != 0;
                self.sweep_shift = val & 0x07;
                self.sweep_reload = true;
            }
            2 => {
                self.timer_period = (self.timer_period & 0xFF00) | val as u16;
            }
            3 => {
                self.timer_period = (self.timer_period & 0x00FF) | ((val as u16 & 0x07) << 8);
                if self.enabled {
                    self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.sequence_pos = 0;
                self.envelope_start = true;
            }
            _ => {}
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter = 0;
        }
    }

    pub fn tick(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            self.sequence_pos = (self.sequence_pos + 1) & 7;
        } else {
            self.timer_counter -= 1;
        }
    }

    pub fn clock_envelope(&mut self) {
        if self.envelope_start {
            self.envelope_start = false;
            self.envelope_decay = 15;
            self.envelope_divider = self.envelope_volume;
        } else if self.envelope_divider == 0 {
            self.envelope_divider = self.envelope_volume;
            if self.envelope_decay > 0 {
                self.envelope_decay -= 1;
            } else if self.length_halt {
                self.envelope_decay = 15;
            }
        } else {
            self.envelope_divider -= 1;
        }
    }

    pub fn clock_length(&mut self) {
        if !self.length_halt && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    pub fn clock_sweep(&mut self) {
        let target = self.sweep_target_period();
        if self.sweep_divider == 0
            && self.sweep_enabled
            && self.sweep_shift > 0
            && self.timer_period >= 8
            && target <= 0x7FF
        {
            self.timer_period = target;
        }
        if self.sweep_divider == 0 || self.sweep_reload {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled
            || self.length_counter == 0
            || DUTY_TABLE[self.duty as usize][self.sequence_pos as usize] == 0
            || self.timer_period < 8
            || self.sweep_target_period() > 0x7FF
        {
            return 0;
        }

        if self.constant_volume {
            self.envelope_volume
        } else {
            self.envelope_decay
        }
    }

    pub fn midi_active(&self) -> bool {
        self.enabled
            && self.length_counter > 0
            && self.timer_period >= 8
            && self.sweep_target_period() <= 0x7FF
    }

    pub fn midi_volume(&self) -> u8 {
        if self.constant_volume {
            self.envelope_volume
        } else {
            self.envelope_decay
        }
    }

    pub fn timer_period(&self) -> u16 {
        self.timer_period
    }

    fn sweep_target_period(&self) -> u16 {
        let shift = self.timer_period >> self.sweep_shift;
        if self.sweep_negate {
            if self.is_pulse1 {
                self.timer_period.saturating_sub(shift).saturating_sub(1)
            } else {
                self.timer_period.saturating_sub(shift)
            }
        } else {
            self.timer_period.wrapping_add(shift)
        }
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bool(self.is_pulse1);
        w.write_bool(self.enabled);
        w.write_u8(self.length_counter);
        w.write_u8(self.duty);
        w.write_bool(self.length_halt);
        w.write_bool(self.constant_volume);
        w.write_u8(self.envelope_volume);
        w.write_bool(self.sweep_enabled);
        w.write_u8(self.sweep_period);
        w.write_bool(self.sweep_negate);
        w.write_u8(self.sweep_shift);
        w.write_bool(self.sweep_reload);
        w.write_u8(self.sweep_divider);
        w.write_u16(self.timer_period);
        w.write_u16(self.timer_counter);
        w.write_u8(self.sequence_pos);
        w.write_bool(self.envelope_start);
        w.write_u8(self.envelope_divider);
        w.write_u8(self.envelope_decay);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.is_pulse1 = r.read_bool()?;
        self.enabled = r.read_bool()?;
        self.length_counter = r.read_u8()?;
        self.duty = r.read_u8()? & 3;
        self.length_halt = r.read_bool()?;
        self.constant_volume = r.read_bool()?;
        self.envelope_volume = r.read_u8()?;
        self.sweep_enabled = r.read_bool()?;
        self.sweep_period = r.read_u8()?;
        self.sweep_negate = r.read_bool()?;
        self.sweep_shift = r.read_u8()?;
        self.sweep_reload = r.read_bool()?;
        self.sweep_divider = r.read_u8()?;
        self.timer_period = r.read_u16()?;
        self.timer_counter = r.read_u16()?;
        self.sequence_pos = r.read_u8()? & 7;
        self.envelope_start = r.read_bool()?;
        self.envelope_divider = r.read_u8()?;
        self.envelope_decay = r.read_u8()?;
        Ok(())
    }
}

#[rustfmt::skip]
pub(crate) static LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6,
    160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

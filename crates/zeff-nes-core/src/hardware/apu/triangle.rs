use super::pulse::LENGTH_TABLE;

pub struct Triangle {
    pub enabled: bool,
    pub length_counter: u8,

    linear_counter: u8,
    linear_counter_reload: u8,
    linear_counter_reload_flag: bool,
    control_flag: bool,

    timer_period: u16,
    timer_counter: u16,
    sequence_pos: u8,
}

#[rustfmt::skip]
static TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

impl Triangle {
    pub fn new() -> Self {
        Self {
            enabled: false,
            length_counter: 0,
            linear_counter: 0,
            linear_counter_reload: 0,
            linear_counter_reload_flag: false,
            control_flag: false,
            timer_period: 0,
            timer_counter: 0,
            sequence_pos: 0,
        }
    }

    pub fn write(&mut self, offset: u16, val: u8) {
        match offset {
            0 => {
                self.control_flag = val & 0x80 != 0;
                self.linear_counter_reload = val & 0x7F;
            }
            2 => {
                self.timer_period = (self.timer_period & 0xFF00) | val as u16;
            }
            3 => {
                self.timer_period = (self.timer_period & 0x00FF) | ((val as u16 & 0x07) << 8);
                if self.enabled {
                    self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.linear_counter_reload_flag = true;
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

    /// Called every CPU cycle.
    #[inline]
    pub fn tick(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            if self.length_counter > 0 && self.linear_counter > 0 {
                self.sequence_pos = (self.sequence_pos + 1) & 31;
            }
        } else {
            self.timer_counter -= 1;
        }
    }

    pub fn clock_linear_counter(&mut self) {
        if self.linear_counter_reload_flag {
            self.linear_counter = self.linear_counter_reload;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }
        if !self.control_flag {
            self.linear_counter_reload_flag = false;
        }
    }

    pub fn clock_length(&mut self) {
        if !self.control_flag && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    #[inline]
    pub fn output(&self) -> u8 {
        TRIANGLE_SEQUENCE[self.sequence_pos as usize]
    }

    pub fn midi_active(&self) -> bool {
        self.enabled && self.length_counter > 0 && self.linear_counter > 0
    }

    pub fn midi_volume(&self) -> u8 {
        15
    }

    pub fn timer_period(&self) -> u16 {
        self.timer_period
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bool(self.enabled);
        w.write_u8(self.length_counter);
        w.write_u8(self.linear_counter);
        w.write_u8(self.linear_counter_reload);
        w.write_bool(self.linear_counter_reload_flag);
        w.write_bool(self.control_flag);
        w.write_u16(self.timer_period);
        w.write_u16(self.timer_counter);
        w.write_u8(self.sequence_pos);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.enabled = r.read_bool()?;
        self.length_counter = r.read_u8()?;
        self.linear_counter = r.read_u8()?;
        self.linear_counter_reload = r.read_u8()?;
        self.linear_counter_reload_flag = r.read_bool()?;
        self.control_flag = r.read_bool()?;
        self.timer_period = r.read_u16()?;
        self.timer_counter = r.read_u16()?;
        self.sequence_pos = r.read_u8()? & 31;
        Ok(())
    }
}

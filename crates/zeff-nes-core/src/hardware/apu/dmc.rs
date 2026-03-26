pub struct Dmc {
    pub enabled: bool,
    pub irq_enabled: bool,
    pub irq_flag: bool,
    pub loop_flag: bool,

    rate_index: u8,
    timer_period: u16,
    timer_counter: u16,

    pub output_level: u8,

    sample_address: u16,
    current_address: u16,
    sample_length: u16,
    pub bytes_remaining: u16,

    shift_register: u8,
    bits_remaining: u8,
    sample_buffer: Option<u8>,
    silence_flag: bool,
}

#[rustfmt::skip]
static DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214,
    190, 160, 142, 128, 106, 84, 72, 54,
];

impl Dmc {
    pub fn new() -> Self {
        Self {
            enabled: false,
            irq_enabled: false,
            irq_flag: false,
            loop_flag: false,
            rate_index: 0,
            timer_period: DMC_RATE_TABLE[0],
            timer_counter: DMC_RATE_TABLE[0],
            output_level: 0,
            sample_address: 0xC000,
            current_address: 0xC000,
            sample_length: 1,
            bytes_remaining: 0,
            shift_register: 0,
            bits_remaining: 0,
            sample_buffer: None,
            silence_flag: true,
        }
    }

    pub fn write(&mut self, offset: u16, val: u8) {
        match offset {
            0 => {
                self.irq_enabled = val & 0x80 != 0;
                self.loop_flag = val & 0x40 != 0;
                self.rate_index = val & 0x0F;
                self.timer_period = DMC_RATE_TABLE[self.rate_index as usize];
                if !self.irq_enabled {
                    self.irq_flag = false;
                }
            }
            1 => {
                self.output_level = val & 0x7F;
            }
            2 => {
                self.sample_address = 0xC000 | ((val as u16) << 6);
            }
            3 => {
                self.sample_length = ((val as u16) << 4) | 1;
            }
            _ => {}
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.irq_flag = false;
        if !enabled {
            self.bytes_remaining = 0;
        } else if self.bytes_remaining == 0 {
            self.current_address = self.sample_address;
            self.bytes_remaining = self.sample_length;
        }
    }

    /// TODO: hook this up to bus DMA reads for proper sample fetching.
    pub fn tick(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            self.tick_output_unit();
        } else {
            self.timer_counter -= 1;
        }
    }

    fn tick_output_unit(&mut self) {
        if !self.silence_flag {
            if self.shift_register & 1 != 0 {
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else if self.output_level >= 2 {
                self.output_level -= 2;
            }
            self.shift_register >>= 1;
        }

        self.bits_remaining = self.bits_remaining.saturating_sub(1);
        if self.bits_remaining == 0 {
            self.bits_remaining = 8;
            if let Some(buf) = self.sample_buffer.take() {
                self.silence_flag = false;
                self.shift_register = buf;
            } else {
                self.silence_flag = true;
            }
        }
    }

    pub fn output(&self) -> u8 {
        self.output_level
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bool(self.enabled);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_flag);
        w.write_bool(self.loop_flag);
        w.write_u8(self.rate_index);
        w.write_u16(self.timer_period);
        w.write_u16(self.timer_counter);
        w.write_u8(self.output_level);
        w.write_u16(self.sample_address);
        w.write_u16(self.current_address);
        w.write_u16(self.sample_length);
        w.write_u16(self.bytes_remaining);
        w.write_u8(self.shift_register);
        w.write_u8(self.bits_remaining);
        w.write_bool(self.sample_buffer.is_some());
        w.write_u8(self.sample_buffer.unwrap_or(0));
        w.write_bool(self.silence_flag);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.enabled = r.read_bool()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_flag = r.read_bool()?;
        self.loop_flag = r.read_bool()?;
        self.rate_index = r.read_u8()?;
        self.timer_period = r.read_u16()?;
        self.timer_counter = r.read_u16()?;
        self.output_level = r.read_u8()?;
        self.sample_address = r.read_u16()?;
        self.current_address = r.read_u16()?;
        self.sample_length = r.read_u16()?;
        self.bytes_remaining = r.read_u16()?;
        self.shift_register = r.read_u8()?;
        self.bits_remaining = r.read_u8()?;
        let has_buf = r.read_bool()?;
        let buf_val = r.read_u8()?;
        self.sample_buffer = if has_buf { Some(buf_val) } else { None };
        self.silence_flag = r.read_bool()?;
        Ok(())
    }
}

use crate::hardware::timing::NesTiming;

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
    timing: NesTiming,
}

#[rustfmt::skip]
const NTSC_DMC_TIMER_PERIOD_CPU_CYCLES: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214,
    190, 160, 142, 128, 106, 84, 72, 54,
];

#[rustfmt::skip]
const PAL_DMC_TIMER_PERIOD_CPU_CYCLES: [u16; 16] = [
    398, 354, 316, 298, 276, 236, 210, 198,
    176, 148, 132, 118, 98, 78, 66, 50,
];

impl Dmc {
    pub fn new() -> Self {
        Self::new_with_timing(NesTiming::Ntsc)
    }

    pub(crate) fn new_with_timing(timing: NesTiming) -> Self {
        let timer_period = match timing {
            NesTiming::Pal => PAL_DMC_TIMER_PERIOD_CPU_CYCLES[0],
            NesTiming::Ntsc | NesTiming::Dendy => NTSC_DMC_TIMER_PERIOD_CPU_CYCLES[0],
        };
        Self {
            enabled: false,
            irq_enabled: false,
            irq_flag: false,
            loop_flag: false,
            rate_index: 0,
            timer_period,
            timer_counter: timer_period - 1,
            output_level: 0,
            sample_address: 0xC000,
            current_address: 0xC000,
            sample_length: 1,
            bytes_remaining: 0,
            shift_register: 0,
            bits_remaining: 0,
            sample_buffer: None,
            silence_flag: true,
            timing,
        }
    }

    pub fn write(&mut self, offset: u16, val: u8) {
        match offset {
            0 => {
                self.irq_enabled = val & 0x80 != 0;
                self.loop_flag = val & 0x40 != 0;
                self.rate_index = val & 0x0F;
                let periods = match self.timing {
                    NesTiming::Pal => &PAL_DMC_TIMER_PERIOD_CPU_CYCLES,
                    NesTiming::Ntsc | NesTiming::Dendy => &NTSC_DMC_TIMER_PERIOD_CPU_CYCLES,
                };
                self.timer_period = periods[self.rate_index as usize];
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

    #[inline]
    pub fn tick(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period.saturating_sub(1);
            self.tick_output_unit();
        } else {
            self.timer_counter -= 1;
        }
    }

    pub fn needs_dma(&self) -> bool {
        self.sample_buffer.is_none() && self.bytes_remaining > 0
    }

    pub fn dma_address(&self) -> u16 {
        self.current_address
    }

    pub fn fill_sample_buffer(&mut self, byte: u8) {
        self.sample_buffer = Some(byte);
        self.current_address = if self.current_address == 0xFFFF {
            0x8000
        } else {
            self.current_address.wrapping_add(1)
        };
        self.bytes_remaining -= 1;
        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.current_address = self.sample_address;
                self.bytes_remaining = self.sample_length;
            } else if self.irq_enabled {
                self.irq_flag = true;
            }
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

    #[inline]
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
        self.rate_index = r.read_u8()? & 0x0F;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_ticks_output_unit_after_exact_period_cycles() {
        let mut dmc = Dmc::new();
        dmc.timer_period = 4;
        dmc.timer_counter = 3;
        dmc.bits_remaining = 8;
        dmc.silence_flag = true;

        dmc.tick();
        dmc.tick();
        dmc.tick();
        assert_eq!(dmc.bits_remaining, 8);
        assert_eq!(dmc.timer_counter, 0);

        dmc.tick();
        assert_eq!(dmc.bits_remaining, 7);
        assert_eq!(dmc.timer_counter, 3);
    }

    #[test]
    fn region_selects_dmc_timer_periods() {
        let mut ntsc = Dmc::new_with_timing(NesTiming::Ntsc);
        let mut pal = Dmc::new_with_timing(NesTiming::Pal);
        let mut dendy = Dmc::new_with_timing(NesTiming::Dendy);

        for index in 0..16 {
            ntsc.write(0, index);
            pal.write(0, index);
            dendy.write(0, index);
            assert_eq!(
                ntsc.timer_period,
                NTSC_DMC_TIMER_PERIOD_CPU_CYCLES[index as usize]
            );
            assert_eq!(
                pal.timer_period,
                PAL_DMC_TIMER_PERIOD_CPU_CYCLES[index as usize]
            );
            assert_eq!(
                dendy.timer_period,
                NTSC_DMC_TIMER_PERIOD_CPU_CYCLES[index as usize]
            );
        }
    }
}

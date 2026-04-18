use super::Apu;
use crate::hardware::constants::*;

impl Apu {
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

                self.frame_cycle = if self.five_step_mode || !odd_cycle {
                    0
                } else {
                    1
                };

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
        if self.pulse1.length_counter > 0 {
            status |= 0x01;
        }
        if self.pulse2.length_counter > 0 {
            status |= 0x02;
        }
        if self.triangle.length_counter > 0 {
            status |= 0x04;
        }
        if self.noise.length_counter > 0 {
            status |= 0x08;
        }
        if self.dmc.bytes_remaining > 0 {
            status |= 0x10;
        }
        if self.frame_irq {
            status |= 0x40;
        }
        if self.dmc.irq_flag {
            status |= 0x80;
        }
        self.frame_irq = false;
        status
    }

    pub fn peek_status(&self) -> u8 {
        let mut status = 0u8;
        if self.pulse1.length_counter > 0 {
            status |= 0x01;
        }
        if self.pulse2.length_counter > 0 {
            status |= 0x02;
        }
        if self.triangle.length_counter > 0 {
            status |= 0x04;
        }
        if self.noise.length_counter > 0 {
            status |= 0x08;
        }
        if self.dmc.bytes_remaining > 0 {
            status |= 0x10;
        }
        if self.frame_irq {
            status |= 0x40;
        }
        if self.dmc.irq_flag {
            status |= 0x80;
        }
        status
    }

    #[inline]
    pub fn tick(&mut self) {
        self.triangle.tick();
        self.dmc.tick();

        if self.frame_cycle.is_multiple_of(2) {
            self.pulse1.tick();
            self.pulse2.tick();
            self.noise.tick();
        }

        self.step_frame_counter();
        self.generate_sample();
        self.frame_cycle += 1;
    }

    #[inline]
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
}

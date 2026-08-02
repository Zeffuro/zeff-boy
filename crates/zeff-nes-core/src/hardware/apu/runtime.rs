use super::Apu;
use crate::hardware::constants::*;

impl Apu {
    pub fn reset(&mut self) {
        let frame_counter_value = (if self.five_step_mode { 0x80 } else { 0x00 })
            | (if self.irq_inhibit { 0x40 } else { 0x00 });

        self.pulse1.set_enabled(false);
        self.pulse2.set_enabled(false);
        self.triangle.set_enabled(false);
        self.noise.set_enabled(false);
        self.dmc.set_enabled(false);

        self.frame_irq = false;
        self.frame_irq_repeat = 0;
        self.frame_reset_delay = 0;
        self.frame_cycle = 8;
        self.write_register(0x4017, frame_counter_value, false);
        self.frame_reset_delay = 0;
        self.frame_cycle = 8;
        self.frame_irq = false;
        self.frame_irq_repeat = 0;
        self.frame_irq_suppression_ticks = 0;
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
                self.five_step_mode = val & 0x80 != 0;
                self.irq_inhibit = val & 0x40 != 0;
                if self.irq_inhibit {
                    self.frame_irq = false;
                    self.frame_irq_repeat = 0;
                }
                if self.five_step_mode {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }

                // The frame sequencer reset caused by $4017 is not immediate on
                // hardware; it is delayed by 3 or 4 CPU clocks after the write
                // cycle. This core invokes cpu_write() before ticking the CPU
                // cycles consumed by the instruction, so include the usual
                // store-instruction tail in the delay instead of resetting the
                // sequencer at opcode dispatch time.
                self.frame_reset_delay = if odd_cycle { 8 } else { 7 };
            }
            _ => {}
        }
    }

    pub fn read_status(&mut self) -> u8 {
        self.read_status_with_frame_irq_lookahead(0)
    }

    pub fn read_status_with_frame_irq_lookahead(&mut self, lookahead_cycles: u8) -> u8 {
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
        let visible_future_frame_irq_ticks = self.frame_irq_visible_tick_count(lookahead_cycles);
        let visible_frame_irq_ticks_through_next =
            self.frame_irq_visible_tick_count(lookahead_cycles.saturating_add(1));
        if self.frame_irq || visible_future_frame_irq_ticks > 0 {
            status |= 0x40;
        }
        if self.dmc.irq_flag {
            status |= 0x80;
        }
        self.frame_irq = false;
        if visible_future_frame_irq_ticks > 0 {
            self.frame_irq_suppression_ticks = self
                .frame_irq_suppression_ticks
                .max(visible_frame_irq_ticks_through_next);
        }
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
        let suppress_frame_irq = self.frame_irq_suppression_ticks > 0;
        self.frame_irq_suppression_ticks = self.frame_irq_suppression_ticks.saturating_sub(1);

        if self.frame_irq_repeat > 0 {
            if !self.irq_inhibit && !suppress_frame_irq {
                self.frame_irq = true;
            }
            self.frame_irq_repeat -= 1;
        }

        if self.frame_reset_delay > 0 {
            self.frame_reset_delay -= 1;
            if self.frame_reset_delay == 0 {
                self.frame_cycle = 0;
            }
        }

        if !self.five_step_mode {
            match self.frame_cycle {
                FRAME_STEP_1 | FRAME_STEP_3 => self.clock_quarter_frame(),
                FRAME_STEP_2 => {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }
                FRAME_STEP_4 => {
                    if !self.irq_inhibit && !suppress_frame_irq {
                        self.frame_irq = true;
                    }
                }
                cycle if cycle == FRAME_STEP_4 + 1 => {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                    if !self.irq_inhibit && !suppress_frame_irq {
                        self.frame_irq = true;
                        self.frame_irq_repeat = 1;
                    }
                }
                FRAME_STEP_4_RESET => {
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
                }
                FRAME_STEP_5_RESET => {
                    self.frame_cycle = 0;
                }
                _ => {}
            }
        }
    }

    fn frame_irq_visible_tick_count(&self, lookahead_cycles: u8) -> u8 {
        if self.irq_inhibit || lookahead_cycles == 0 {
            return 0;
        }

        self.frame_future_event_counts(lookahead_cycles)
            .frame_irq_ticks
    }

    fn frame_future_event_counts(&self, lookahead_cycles: u8) -> FrameFutureEventCounts {
        let mut frame_cycle = self.frame_cycle;
        let mut frame_reset_delay = self.frame_reset_delay;
        let mut frame_irq_repeat = self.frame_irq_repeat;
        let mut counts = FrameFutureEventCounts::default();

        for _ in 0..lookahead_cycles {
            if frame_irq_repeat > 0 {
                counts.frame_irq_ticks = counts.frame_irq_ticks.saturating_add(1);
                frame_irq_repeat -= 1;
            }

            if frame_reset_delay > 0 {
                frame_reset_delay -= 1;
                if frame_reset_delay == 0 {
                    frame_cycle = 0;
                }
            }

            if !self.five_step_mode {
                match frame_cycle {
                    FRAME_STEP_4 => {
                        counts.frame_irq_ticks = counts.frame_irq_ticks.saturating_add(1);
                    }
                    cycle if cycle == FRAME_STEP_4 + 1 => {
                        counts.frame_irq_ticks = counts.frame_irq_ticks.saturating_add(1);
                        frame_irq_repeat = 1;
                    }
                    _ => {}
                }
                if frame_cycle == FRAME_STEP_4_RESET {
                    frame_cycle = 0;
                }
            } else {
                if frame_cycle == FRAME_STEP_5_RESET {
                    frame_cycle = 0;
                }
            }

            frame_cycle += 1;
        }

        counts
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

#[derive(Clone, Copy, Debug, Default)]
struct FrameFutureEventCounts {
    frame_irq_ticks: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_step_write_clocks_half_frame_immediately_once() {
        let mut apu = Apu::new(44_100.0);
        apu.write_register(0x4015, 0x01, false);
        apu.write_register(0x4003, 0x18, false);
        assert_eq!(apu.pulse1.length_counter, 2);

        apu.write_register(0x4017, 0x80, false);
        assert_eq!(apu.pulse1.length_counter, 1);

        for _ in 0..8 {
            apu.tick();
        }
        assert_eq!(
            apu.pulse1.length_counter, 1,
            "delayed sequencer reset must not double-clock length"
        );
    }

    #[test]
    fn four_step_write_does_not_clock_half_frame_immediately() {
        let mut apu = Apu::new(44_100.0);
        apu.write_register(0x4015, 0x01, false);
        apu.write_register(0x4003, 0x18, false);

        apu.write_register(0x4017, 0x00, false);
        assert_eq!(apu.pulse1.length_counter, 2);
    }

    #[test]
    fn reset_clears_channel_enables_and_frame_irq_but_keeps_frame_mode() {
        let mut apu = Apu::new(44_100.0);
        apu.write_register(0x4017, 0x80, false);
        apu.write_register(0x4015, 0x0F, false);
        apu.write_register(0x4003, 0x18, false);
        apu.write_register(0x4007, 0x18, false);
        apu.write_register(0x400B, 0x18, false);
        apu.write_register(0x400F, 0x18, false);
        apu.frame_irq = true;

        apu.reset();

        assert!(apu.five_step_mode);
        assert!(!apu.frame_irq);
        assert_eq!(apu.frame_cycle, 8);
        assert_eq!(apu.peek_status() & 0x0F, 0);
    }

    #[test]
    fn reset_preserves_four_step_irq_enable_state() {
        let mut apu = Apu::new(44_100.0);
        apu.write_register(0x4017, 0x00, false);
        apu.frame_irq = true;

        apu.reset();

        assert!(!apu.five_step_mode);
        assert!(!apu.irq_inhibit);
        assert!(!apu.frame_irq);
        assert_eq!(apu.frame_cycle, 8);
    }

    #[test]
    fn four_step_frame_irq_becomes_visible_before_length_clock() {
        let mut apu = Apu::new(44_100.0);
        apu.write_register(0x4015, 0x01, false);
        apu.pulse1.length_counter = 1;
        apu.frame_irq = false;
        apu.frame_irq_repeat = 0;
        apu.frame_reset_delay = 0;
        apu.frame_cycle = FRAME_STEP_4;

        apu.tick();
        assert_eq!(apu.peek_status() & 0x41, 0x41);

        apu.tick();
        assert_eq!(apu.peek_status() & 0x41, 0x40);
    }
}

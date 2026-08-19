use super::Apu;
use crate::hardware::constants::*;

#[derive(Clone, Copy)]
struct FrameClocks {
    quarter: bool,
    half: bool,
}

impl Apu {
    pub fn reset(&mut self) {
        self.pulse1.set_enabled(false);
        self.pulse2.set_enabled(false);
        self.triangle.set_enabled(false);
        self.noise.set_enabled(false);
        self.dmc.set_enabled(false);

        self.frame_irq = false;
        self.frame_reset_delay = 0;
        self.frame_cycle = 9;
        self.pending_frame_counter_value = None;
        self.frame_clock_block = 0;
        self.clock_half_rate_timers = false;
        self.pulse1.clear_length_clock_collision();
        self.pulse2.clear_length_clock_collision();
        self.triangle.clear_length_clock_collision();
        self.noise.clear_length_clock_collision();
    }

    pub fn write_register(&mut self, addr: u16, val: u8, odd_cycle: bool) {
        let length_clock_due = self.length_clock_due_this_tick();
        match addr {
            0x4000..=0x4003 => self.pulse1.write(addr - 0x4000, val, length_clock_due),
            0x4004..=0x4007 => self.pulse2.write(addr - 0x4004, val, length_clock_due),
            0x4008..=0x400B => self.triangle.write(addr - 0x4008, val, length_clock_due),
            0x400C..=0x400F => self.noise.write(addr - 0x400C, val, length_clock_due),
            0x4010..=0x4013 => self.dmc.write(addr - 0x4010, val),
            0x4015 => {
                self.pulse1.set_enabled(val & 0x01 != 0);
                self.pulse2.set_enabled(val & 0x02 != 0);
                self.triangle.set_enabled(val & 0x04 != 0);
                self.noise.set_enabled(val & 0x08 != 0);
                self.dmc.set_enabled(val & 0x10 != 0);
            }
            0x4017 => {
                self.irq_inhibit = val & 0x40 != 0;
                if self.irq_inhibit {
                    self.frame_irq = false;
                }
                self.pending_frame_counter_value = Some(val);
                self.frame_reset_delay = if odd_cycle { 4 } else { 3 };
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

        if self.clock_half_rate_timers {
            self.pulse1.tick();
            self.pulse2.tick();
            self.noise.tick();
        }
        self.clock_half_rate_timers = !self.clock_half_rate_timers;

        self.step_frame_counter();
        self.generate_sample();
        self.frame_cycle += 1;
    }

    #[inline]
    pub fn irq_pending(&self) -> bool {
        self.frame_irq || self.dmc.irq_flag
    }

    fn step_frame_counter(&mut self) {
        if self.run_frame_event() {
            self.frame_clock_block = 2;
        }

        if self.frame_reset_delay > 0 {
            self.frame_reset_delay -= 1;
            if self.frame_reset_delay == 0 {
                let value = self.pending_frame_counter_value.take().unwrap_or(0);
                self.five_step_mode = value & 0x80 != 0;
                self.frame_cycle = 0;
                if self.five_step_mode && self.frame_clock_block == 0 {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                    self.frame_clock_block = 2;
                }
            }
        }

        self.frame_clock_block = self.frame_clock_block.saturating_sub(1);
    }

    fn run_frame_event(&mut self) -> bool {
        let clocks = self.frame_clocks_this_tick();
        if clocks.quarter {
            self.clock_quarter_frame();
        }
        if clocks.half {
            self.clock_half_frame();
        }

        if !self.five_step_mode {
            match self.frame_cycle {
                FRAME_STEP_4_IRQ_START => {
                    if !self.irq_inhibit {
                        self.frame_irq = true;
                    }
                }
                FRAME_STEP_4_CLOCK => {
                    if !self.irq_inhibit {
                        self.frame_irq = true;
                    }
                }
                FRAME_STEP_4_RESET => {
                    if !self.irq_inhibit {
                        self.frame_irq = true;
                    }
                    self.frame_cycle = 0;
                }
                _ => {}
            }
        } else {
            if self.frame_cycle == FRAME_STEP_5_RESET {
                self.frame_cycle = 0;
            }
        }

        clocks.quarter
    }

    fn frame_clocks_this_tick(&self) -> FrameClocks {
        match (self.five_step_mode, self.frame_cycle) {
            (_, FRAME_STEP_1 | FRAME_STEP_3) => FrameClocks {
                quarter: true,
                half: false,
            },
            (false, FRAME_STEP_2 | FRAME_STEP_4_CLOCK) | (true, FRAME_STEP_2 | FRAME_STEP_5) => {
                FrameClocks {
                    quarter: true,
                    half: true,
                }
            }
            _ => FrameClocks {
                quarter: false,
                half: false,
            },
        }
    }

    fn length_clock_due_this_tick(&self) -> bool {
        let clocks = self.frame_clocks_this_tick();
        let delayed_five_step_clock = self.frame_reset_delay == 1
            && self
                .pending_frame_counter_value
                .is_some_and(|value| value & 0x80 != 0)
            && self.frame_clock_block == 0
            && !clocks.quarter;
        clocks.half || delayed_five_step_clock
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

#[cfg(test)]
mod tests {
    use super::*;

    fn length_counter(apu: &Apu, channel: usize) -> u8 {
        match channel {
            0 => apu.pulse1.length_counter,
            1 => apu.pulse2.length_counter,
            2 => apu.triangle.length_counter,
            3 => apu.noise.length_counter,
            _ => unreachable!(),
        }
    }

    fn set_length_counter(apu: &mut Apu, channel: usize, value: u8) {
        match channel {
            0 => apu.pulse1.length_counter = value,
            1 => apu.pulse2.length_counter = value,
            2 => apu.triangle.length_counter = value,
            3 => apu.noise.length_counter = value,
            _ => unreachable!(),
        }
    }

    #[test]
    fn five_step_write_clocks_half_frame_immediately_once() {
        let mut apu = Apu::new(44_100.0);
        apu.write_register(0x4015, 0x01, false);
        apu.write_register(0x4003, 0x18, false);
        assert_eq!(apu.pulse1.length_counter, 2);

        apu.write_register(0x4017, 0x80, false);
        assert_eq!(apu.pulse1.length_counter, 2);

        for _ in 0..3 {
            apu.tick();
        }
        assert_eq!(apu.pulse1.length_counter, 1);

        for _ in 0..5 {
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
    fn frame_event_precedes_a_colliding_delayed_write() {
        let mut apu = Apu::new(44_100.0);
        apu.write_register(0x4015, 0x01, false);
        apu.write_register(0x4003, 0x18, false);
        apu.write_register(0x4017, 0x80, false);
        apu.frame_cycle = FRAME_STEP_2;
        apu.frame_reset_delay = 1;

        apu.tick();

        assert!(apu.five_step_mode);
        assert_eq!(apu.pulse1.length_counter, 1);
    }

    #[test]
    fn frame_counter_reset_does_not_reset_half_rate_timer_phase() {
        let mut apu = Apu::new(44_100.0);
        let initial_phase = apu.clock_half_rate_timers;
        apu.write_register(0x4017, 0x80, false);

        for _ in 0..3 {
            apu.tick();
        }

        assert_eq!(apu.clock_half_rate_timers, !initial_phase);
    }

    #[test]
    fn length_reload_collision_uses_the_pre_clock_counter() {
        for (channel, address) in [0x4003, 0x4007, 0x400B, 0x400F].into_iter().enumerate() {
            let mut apu = Apu::new(44_100.0);
            apu.write_register(0x4015, 0x0F, false);
            set_length_counter(&mut apu, channel, 6);
            apu.frame_cycle = FRAME_STEP_2;

            apu.write_register(address, 0x18, false);
            apu.tick();

            assert_eq!(length_counter(&apu, channel), 5, "channel {channel}");
        }
    }

    #[test]
    fn zero_length_reload_collision_skips_the_same_clock() {
        for (channel, address) in [0x4003, 0x4007, 0x400B, 0x400F].into_iter().enumerate() {
            let mut apu = Apu::new(44_100.0);
            apu.write_register(0x4015, 0x0F, false);
            apu.frame_cycle = FRAME_STEP_2;

            apu.write_register(address, 0x18, false);
            apu.tick();

            assert_eq!(length_counter(&apu, channel), 2, "channel {channel}");
        }
    }

    #[test]
    fn length_halt_collision_uses_the_previous_flag() {
        let control_writes = [
            (0x4000, 0x20),
            (0x4004, 0x20),
            (0x4008, 0x80),
            (0x400C, 0x20),
        ];
        for (channel, (address, halt_value)) in control_writes.into_iter().enumerate() {
            let mut apu = Apu::new(44_100.0);
            set_length_counter(&mut apu, channel, 2);
            apu.frame_cycle = FRAME_STEP_2;
            apu.write_register(address, halt_value, false);
            apu.tick();
            assert_eq!(length_counter(&apu, channel), 1, "halt channel {channel}");

            set_length_counter(&mut apu, channel, 2);
            apu.frame_cycle = FRAME_STEP_2;
            apu.write_register(address, 0, false);
            apu.tick();
            assert_eq!(length_counter(&apu, channel), 2, "resume channel {channel}");
        }
    }

    #[test]
    fn status_write_does_not_clear_frame_irq() {
        let mut apu = Apu::new(44_100.0);
        apu.frame_irq = true;

        apu.write_register(0x4015, 0, false);

        assert!(apu.frame_irq);
    }

    #[test]
    fn reset_clears_channel_enables_and_frame_irq_but_keeps_frame_mode() {
        let mut apu = Apu::new(44_100.0);
        apu.write_register(0x4017, 0x80, false);
        for _ in 0..3 {
            apu.tick();
        }
        apu.write_register(0x4015, 0x0F, false);
        apu.write_register(0x4003, 0x18, false);
        apu.write_register(0x4007, 0x18, false);
        apu.write_register(0x400B, 0x18, false);
        apu.write_register(0x400F, 0x18, false);
        apu.frame_irq = true;

        apu.reset();

        assert!(apu.five_step_mode);
        assert!(!apu.frame_irq);
        assert_eq!(apu.frame_cycle, 9);
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
        assert_eq!(apu.frame_cycle, 9);
    }

    #[test]
    fn four_step_frame_irq_becomes_visible_before_length_clock() {
        let mut apu = Apu::new(44_100.0);
        apu.write_register(0x4015, 0x01, false);
        apu.pulse1.length_counter = 1;
        apu.frame_irq = false;
        apu.frame_reset_delay = 0;
        apu.frame_cycle = FRAME_STEP_4_IRQ_START;

        apu.tick();
        assert_eq!(apu.peek_status() & 0x41, 0x41);

        apu.tick();
        assert_eq!(apu.peek_status() & 0x41, 0x40);
    }
}

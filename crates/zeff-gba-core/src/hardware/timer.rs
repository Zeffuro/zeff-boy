#[derive(Clone, Copy, Debug, Default)]
pub struct Timer {
    pub reload: u16,
    pub counter: u16,
    pub control: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timers {
    timers: [Timer; 4],
    cycle_accum: [u32; 4],
}

pub type TimerOverflowCounts = [u32; 4];

impl Timers {
    pub fn read16(&self, index: usize, control: bool) -> u16 {
        let timer = self.timers.get(index).copied().unwrap_or_default();
        if control {
            timer.control
        } else {
            timer.counter
        }
    }

    pub fn write16(&mut self, index: usize, control: bool, value: u16) {
        if let Some(timer) = self.timers.get_mut(index) {
            if control {
                let old_control = timer.control;
                timer.control = value & 0x00C7;
                if old_control & 0x0080 == 0 && timer.control & 0x0080 != 0 {
                    timer.counter = timer.reload;
                    if let Some(accum) = self.cycle_accum.get_mut(index) {
                        *accum = 0;
                    }
                }
            } else {
                timer.reload = value;
            }
        }
    }

    pub fn step(&mut self, cycles: u32) -> u16 {
        self.step_with_overflows(cycles).0
    }

    pub fn step_with_overflows(&mut self, cycles: u32) -> (u16, TimerOverflowCounts) {
        let mut irq_flags = 0u16;
        let mut overflow_counts = [0u32; 4];
        for index in 0..4 {
            let timer = self.timers[index];
            if timer.control & 0x0080 == 0 || timer.control & 0x0004 != 0 {
                continue;
            }

            self.cycle_accum[index] = self.cycle_accum[index].saturating_add(cycles);
            let period = timer_period(timer.control);
            while self.cycle_accum[index] >= period {
                self.cycle_accum[index] -= period;
                if self.increment_timer(index, &mut irq_flags, &mut overflow_counts) {
                    self.increment_cascade(index + 1, &mut irq_flags, &mut overflow_counts);
                }
            }
        }
        (irq_flags, overflow_counts)
    }

    pub fn all(&self) -> [Timer; 4] {
        self.timers
    }

    pub fn cycles_until_overflow(&self, index: usize) -> Option<u32> {
        let timer = self.timers.get(index).copied()?;
        if timer.control & 0x0080 == 0 || timer.control & 0x0004 != 0 {
            return None;
        }
        let period = timer_period(timer.control);
        let accum = self.cycle_accum.get(index).copied().unwrap_or(0);
        let cycles_until_increment = period.saturating_sub(accum).max(1);
        let increments_until_overflow = 0x1_0000 - u32::from(timer.counter);
        Some(
            cycles_until_increment.saturating_add(
                increments_until_overflow
                    .saturating_sub(1)
                    .saturating_mul(period),
            ),
        )
    }

    pub fn set_all(&mut self, timers: [Timer; 4]) {
        self.timers = timers;
        self.cycle_accum = [0; 4];
    }

    fn increment_cascade(
        &mut self,
        index: usize,
        irq_flags: &mut u16,
        overflow_counts: &mut TimerOverflowCounts,
    ) {
        if index >= self.timers.len() {
            return;
        }
        let timer = self.timers[index];
        if timer.control & 0x0080 == 0 || timer.control & 0x0004 == 0 {
            return;
        }
        if self.increment_timer(index, irq_flags, overflow_counts) {
            self.increment_cascade(index + 1, irq_flags, overflow_counts);
        }
    }

    fn increment_timer(
        &mut self,
        index: usize,
        irq_flags: &mut u16,
        overflow_counts: &mut TimerOverflowCounts,
    ) -> bool {
        let timer = &mut self.timers[index];
        let (counter, overflowed) = timer.counter.overflowing_add(1);
        if overflowed {
            timer.counter = timer.reload;
            overflow_counts[index] = overflow_counts[index].saturating_add(1);
            if timer.control & 0x0040 != 0 {
                *irq_flags |= 1 << (3 + index);
            }
            true
        } else {
            timer.counter = counter;
            false
        }
    }
}

fn timer_period(control: u16) -> u32 {
    match control & 0x0003 {
        0 => 1,
        1 => 64,
        2 => 256,
        _ => 1024,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_copies_reload_to_counter() {
        let mut timers = Timers::default();
        timers.write16(0, false, 0x1234);

        assert_eq!(timers.read16(0, false), 0);

        timers.write16(0, true, 0x0080);

        assert_eq!(timers.read16(0, false), 0x1234);
    }

    #[test]
    fn timer_overflow_reloads_and_sets_irq_flag() {
        let mut timers = Timers::default();
        timers.write16(0, false, 0xFFFF);
        timers.write16(0, true, 0x00C0);

        let flags = timers.step(1);

        assert_eq!(timers.read16(0, false), 0xFFFF);
        assert_eq!(flags, 1 << 3);
    }

    #[test]
    fn cascade_timer_increments_on_previous_overflow() {
        let mut timers = Timers::default();
        timers.write16(0, false, 0xFFFF);
        timers.write16(1, false, 0xFFFE);
        timers.write16(0, true, 0x0080);
        timers.write16(1, true, 0x0084);

        timers.step(1);
        assert_eq!(timers.read16(1, false), 0xFFFF);

        timers.step(1);
        assert_eq!(timers.read16(1, false), 0xFFFE);
    }

    #[test]
    fn step_reports_each_overflow_for_fast_timers() {
        let mut timers = Timers::default();
        timers.write16(0, false, 0xFFFF);
        timers.write16(0, true, 0x0080);

        let (flags, overflows) = timers.step_with_overflows(4);

        assert_eq!(flags, 0);
        assert_eq!(overflows[0], 4);
        assert_eq!(timers.read16(0, false), 0xFFFF);
    }

    #[test]
    fn cycles_until_overflow_accounts_for_prescaler_and_counter() {
        let mut timers = Timers::default();
        timers.write16(0, false, 0xFFFE);
        timers.write16(0, true, 0x0081);

        assert_eq!(timers.cycles_until_overflow(0), Some(128));

        timers.step(32);
        assert_eq!(timers.cycles_until_overflow(0), Some(96));

        timers.step(96);
        assert_eq!(timers.read16(0, false), 0xFFFE);
        assert_eq!(timers.cycles_until_overflow(0), Some(128));
    }
}

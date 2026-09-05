#[derive(Clone, Copy, Debug, Default)]
pub struct Timer {
    pub reload: u16,
    pub counter: u16,
    pub control: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timers {
    timers: [Timer; 4],
    // Only enabled, non-cascading timers consume CPU cycles. Keeping this derived
    // mask avoids walking the four disabled timer registers at every instruction.
    clocked_timer_mask: u8,
    cycle_accum: [u32; 4],
    start_delay_cycles: [u8; 4],
    clock_phase: u16,
}

pub type TimerOverflowCounts = [u32; 4];
pub type TimerIrqExtraDelays = [u32; 4];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TimerTimingState {
    pub cycle_accum: [u32; 4],
    pub start_delay_cycles: [u8; 4],
    pub clock_phase: u16,
}

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
                        let period = timer_period(timer.control);
                        *accum = u32::from(self.clock_phase.wrapping_add(1)) & (period - 1);
                    }
                    self.start_delay_cycles[index] = 1;
                } else if timer.control & 0x0080 == 0 {
                    self.start_delay_cycles[index] = 0;
                }
                self.refresh_clocked_timer_mask(index);
            } else {
                timer.reload = value;
            }
        }
    }

    pub fn step(&mut self, cycles: u32) -> u16 {
        self.step_with_overflows(cycles).0
    }

    pub fn step_with_overflows(
        &mut self,
        cycles: u32,
    ) -> (u16, TimerOverflowCounts, TimerIrqExtraDelays) {
        self.clock_phase = self.clock_phase.wrapping_add(cycles as u16) & 0x03FF;
        if self.clocked_timer_mask == 0 {
            return (0, [0; 4], [0; 4]);
        }
        let mut irq_flags = 0u16;
        let mut overflow_counts = [0u32; 4];
        let mut irq_extra_delays = [0u32; 4];
        for index in 0..4 {
            let timer = self.timers[index];
            if timer.control & 0x0080 == 0 || timer.control & 0x0004 != 0 {
                continue;
            }

            let delay = u32::from(self.start_delay_cycles[index]).min(cycles);
            self.start_delay_cycles[index] -= delay as u8;
            let count_cycles = cycles - delay;
            if count_cycles == 0 {
                continue;
            }
            self.cycle_accum[index] = self.cycle_accum[index].saturating_add(count_cycles);
            let period = timer_period(timer.control);
            while self.cycle_accum[index] >= period {
                self.cycle_accum[index] -= period;
                if self.increment_timer(
                    index,
                    &mut irq_flags,
                    &mut overflow_counts,
                    &mut irq_extra_delays,
                ) {
                    self.increment_cascade(
                        index + 1,
                        &mut irq_flags,
                        &mut overflow_counts,
                        &mut irq_extra_delays,
                    );
                }
            }
        }
        (irq_flags, overflow_counts, irq_extra_delays)
    }

    pub fn all(&self) -> [Timer; 4] {
        self.timers
    }

    pub(crate) fn timing_state(&self) -> TimerTimingState {
        TimerTimingState {
            cycle_accum: self.cycle_accum,
            start_delay_cycles: self.start_delay_cycles,
            clock_phase: self.clock_phase,
        }
    }

    pub(crate) fn set_timing_state(&mut self, state: TimerTimingState) -> bool {
        if state.clock_phase > 0x03FF
            || state.cycle_accum.into_iter().any(|accum| accum > 0x03FF)
            || state.start_delay_cycles.into_iter().any(|delay| delay > 1)
        {
            return false;
        }
        self.cycle_accum = state.cycle_accum;
        self.start_delay_cycles = state.start_delay_cycles;
        self.clock_phase = state.clock_phase;
        true
    }

    pub(crate) fn migrate_legacy_timing(&mut self, cycles: u64) {
        let phase = (cycles as u16) & 0x03FF;
        self.clock_phase = phase;
        self.start_delay_cycles = [0; 4];
        for (index, timer) in self.timers.iter().enumerate() {
            self.cycle_accum[index] = if timer.control & 0x0084 == 0x0080 {
                u32::from(phase) & (timer_period(timer.control) - 1)
            } else {
                0
            };
        }
    }

    pub fn cycles_until_overflow(&self, index: usize) -> Option<u32> {
        if index >= self.timers.len() {
            return None;
        }
        if self.clocked_timer_mask & (1 << index) == 0 {
            return None;
        }
        let timer = self.timers.get(index).copied()?;
        let period = timer_period(timer.control);
        let accum = self.cycle_accum.get(index).copied().unwrap_or(0);
        let start_delay = u32::from(self.start_delay_cycles[index]);
        let increments_until_overflow = 0x1_0000 - u32::from(timer.counter);
        Some(
            start_delay.saturating_add(
                increments_until_overflow
                    .saturating_mul(period)
                    .saturating_sub(accum)
                    .max(1),
            ),
        )
    }

    pub fn set_all(&mut self, timers: [Timer; 4]) {
        self.timers = timers;
        self.clocked_timer_mask = self
            .timers
            .iter()
            .enumerate()
            .fold(0, |mask, (index, timer)| {
                mask | (u8::from(timer.control & 0x0084 == 0x0080) << index)
            });
        self.cycle_accum = [0; 4];
        self.start_delay_cycles = [0; 4];
        self.clock_phase = 0;
    }

    #[inline]
    pub(crate) const fn has_clocked_timers(&self) -> bool {
        self.clocked_timer_mask != 0
    }

    #[inline]
    fn refresh_clocked_timer_mask(&mut self, index: usize) {
        let bit = 1 << index;
        if self.timers[index].control & 0x0084 == 0x0080 {
            self.clocked_timer_mask |= bit;
        } else {
            self.clocked_timer_mask &= !bit;
        }
    }

    fn increment_cascade(
        &mut self,
        index: usize,
        irq_flags: &mut u16,
        overflow_counts: &mut TimerOverflowCounts,
        irq_extra_delays: &mut TimerIrqExtraDelays,
    ) {
        if index >= self.timers.len() {
            return;
        }
        let timer = self.timers[index];
        if timer.control & 0x0080 == 0 || timer.control & 0x0004 == 0 {
            return;
        }
        if self.increment_timer(index, irq_flags, overflow_counts, irq_extra_delays) {
            self.increment_cascade(index + 1, irq_flags, overflow_counts, irq_extra_delays);
        }
    }

    fn increment_timer(
        &mut self,
        index: usize,
        irq_flags: &mut u16,
        overflow_counts: &mut TimerOverflowCounts,
        irq_extra_delays: &mut TimerIrqExtraDelays,
    ) -> bool {
        let timer = &mut self.timers[index];
        let (counter, overflowed) = timer.counter.overflowing_add(1);
        if overflowed {
            timer.counter = timer.reload;
            overflow_counts[index] = overflow_counts[index].saturating_add(1);
            if timer.control & 0x0040 != 0 {
                *irq_flags |= 1 << (3 + index);
                irq_extra_delays[index] = 0;
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
    fn scheduler_mask_tracks_enabled_non_cascading_timers() {
        let mut timers = Timers::default();
        assert!(!timers.has_clocked_timers());

        timers.write16(1, true, 0x0084);
        assert!(!timers.has_clocked_timers());

        timers.write16(0, true, 0x0080);
        assert!(timers.has_clocked_timers());

        timers.write16(0, true, 0);
        assert!(!timers.has_clocked_timers());

        let mut restored = [Timer::default(); 4];
        restored[2].control = 0x0080;
        timers.set_all(restored);
        assert!(timers.has_clocked_timers());
    }

    #[test]
    fn timer_overflow_reloads_and_sets_irq_flag() {
        let mut timers = Timers::default();
        timers.write16(0, false, 0xFFFF);
        timers.write16(0, true, 0x00C0);

        let flags = timers.step(1);

        assert_eq!(flags, 0);
        assert_eq!(timers.read16(0, false), 0xFFFF);

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
        assert_eq!(timers.read16(1, false), 0xFFFE);

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

        let (flags, overflows, _) = timers.step_with_overflows(4);

        assert_eq!(flags, 0);
        assert_eq!(overflows[0], 3);
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

    #[test]
    fn cycles_until_overflow_accounts_for_accepted_noncanonical_accumulator() {
        let mut timers = Timers::default();
        timers.write16(0, false, 0xFFF0);
        timers.write16(0, true, 0x00C0);
        assert!(timers.set_timing_state(TimerTimingState {
            cycle_accum: [31, 0, 0, 0],
            start_delay_cycles: [1, 0, 0, 0],
            clock_phase: 0,
        }));

        assert_eq!(timers.cycles_until_overflow(0), Some(2));
        assert_eq!(timers.step_with_overflows(1).1[0], 0);
        assert_eq!(timers.step_with_overflows(1).1[0], 2);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timer {
    pub reload: u16,
    pub counter: u16,
    pub control: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timers {
    timers: [Timer; 4],
    // Only timers eligible for CPU clocks consume CPU cycles. TM0 ignores its
    // readable count-up bit; TM1-TM3 use it to select cascade clocks.
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
            if !timer_uses_cpu_clock(index, timer.control) {
                continue;
            }

            let delay = u32::from(self.start_delay_cycles[index]).min(cycles);
            self.start_delay_cycles[index] -= delay as u8;
            let count_cycles = cycles - delay;
            if count_cycles == 0 {
                continue;
            }
            let period = timer_period(timer.control);
            let total = (u64::from(self.cycle_accum[index]) + u64::from(count_cycles))
                .min(u64::from(u32::MAX));
            let increments = u32::try_from(total / u64::from(period)).unwrap_or(u32::MAX);
            self.cycle_accum[index] = u32::try_from(total % u64::from(period)).unwrap_or_default();
            let cascades = self.increment_timer_by(
                index,
                increments,
                &mut irq_flags,
                &mut overflow_counts,
                &mut irq_extra_delays,
            );
            if cascades != 0 {
                self.increment_cascade_by(
                    index + 1,
                    cascades,
                    &mut irq_flags,
                    &mut overflow_counts,
                    &mut irq_extra_delays,
                );
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
            self.cycle_accum[index] = if timer_uses_cpu_clock(index, timer.control) {
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
                mask | (u8::from(timer_uses_cpu_clock(index, timer.control)) << index)
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
        if timer_uses_cpu_clock(index, self.timers[index].control) {
            self.clocked_timer_mask |= bit;
        } else {
            self.clocked_timer_mask &= !bit;
        }
    }

    fn increment_cascade_by(
        &mut self,
        index: usize,
        increments: u32,
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
        let cascades = self.increment_timer_by(
            index,
            increments,
            irq_flags,
            overflow_counts,
            irq_extra_delays,
        );
        if cascades != 0 {
            self.increment_cascade_by(
                index + 1,
                cascades,
                irq_flags,
                overflow_counts,
                irq_extra_delays,
            );
        }
    }

    fn increment_timer_by(
        &mut self,
        index: usize,
        increments: u32,
        irq_flags: &mut u16,
        overflow_counts: &mut TimerOverflowCounts,
        irq_extra_delays: &mut TimerIrqExtraDelays,
    ) -> u32 {
        if increments == 0 {
            return 0;
        }
        let timer = &mut self.timers[index];
        let increments = u64::from(increments);
        let first_overflow = 0x1_0000 - u64::from(timer.counter);
        if increments < first_overflow {
            timer.counter = timer.counter.wrapping_add(increments as u16);
            return 0;
        }

        let remaining = increments - first_overflow;
        let reload_span = 0x1_0000 - u64::from(timer.reload);
        let overflows = 1 + remaining / reload_span;
        timer.counter = timer.reload.wrapping_add((remaining % reload_span) as u16);
        let overflows = u32::try_from(overflows).unwrap_or(u32::MAX);
        overflow_counts[index] = overflow_counts[index].saturating_add(overflows);
        if timer.control & 0x0040 != 0 {
            *irq_flags |= 1 << (3 + index);
            irq_extra_delays[index] = 0;
        }
        overflows
    }

    #[cfg(test)]
    fn step_with_overflows_scalar(
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
            if timer.control & 0x0080 == 0 || (index != 0 && timer.control & 0x0004 != 0) {
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
                if self.increment_timer_scalar(
                    index,
                    &mut irq_flags,
                    &mut overflow_counts,
                    &mut irq_extra_delays,
                ) {
                    self.increment_cascade_scalar(
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

    #[cfg(test)]
    fn increment_cascade_scalar(
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
        if self.increment_timer_scalar(index, irq_flags, overflow_counts, irq_extra_delays) {
            self.increment_cascade_scalar(index + 1, irq_flags, overflow_counts, irq_extra_delays);
        }
    }

    #[cfg(test)]
    fn increment_timer_scalar(
        &mut self,
        index: usize,
        irq_flags: &mut u16,
        overflow_counts: &mut TimerOverflowCounts,
        irq_extra_delays: &mut TimerIrqExtraDelays,
    ) -> bool {
        let timer = &mut self.timers[index];
        let (counter, overflowed) = timer.counter.overflowing_add(1);
        if !overflowed {
            timer.counter = counter;
            return false;
        }
        timer.counter = timer.reload;
        overflow_counts[index] = overflow_counts[index].saturating_add(1);
        if timer.control & 0x0040 != 0 {
            *irq_flags |= 1 << (3 + index);
            irq_extra_delays[index] = 0;
        }
        true
    }
}

fn timer_uses_cpu_clock(index: usize, control: u16) -> bool {
    control & 0x0080 != 0 && (index == 0 || control & 0x0004 == 0)
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

    fn assert_timer_state_eq(first: &Timers, second: &Timers) {
        for index in 0..4 {
            assert_eq!(first.read16(index, false), second.read16(index, false));
            assert_eq!(first.read16(index, true), second.read16(index, true));
        }
        assert_eq!(first.timing_state(), second.timing_state());
        assert_eq!(first.clocked_timer_mask, second.clocked_timer_mask);
    }

    fn assert_bulk_matches_scalar(source: Timers, cycles: u32) {
        let mut bulk = source;
        let mut scalar = source;
        assert_eq!(
            bulk.step_with_overflows(cycles),
            scalar.step_with_overflows_scalar(cycles)
        );
        assert_timer_state_eq(&bulk, &scalar);
    }

    fn next_random(state: &mut u64) -> u32 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*state >> 32) as u32
    }

    fn random_timers(state: &mut u64) -> Timers {
        let mut registers = [Timer::default(); 4];
        for timer in &mut registers {
            let control = next_random(state);
            *timer = Timer {
                reload: next_random(state) as u16,
                counter: next_random(state) as u16,
                control: (control as u16 & 0x0003)
                    | (u16::from(control & (1 << 8) != 0) << 2)
                    | (u16::from(control & (1 << 9) != 0) << 6)
                    | (u16::from(control & (1 << 10) != 0) << 7),
            };
        }
        let mut timers = Timers::default();
        timers.set_all(registers);
        let mut timing = TimerTimingState::default();
        for index in 0..4 {
            timing.cycle_accum[index] = next_random(state) & 0x03FF;
            timing.start_delay_cycles[index] = (next_random(state) & 1) as u8;
        }
        timing.clock_phase = (next_random(state) & 0x03FF) as u16;
        assert!(timers.set_timing_state(timing));
        timers
    }

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

    #[test]
    fn bulk_step_matches_scalar_for_every_enable_cascade_irq_topology() {
        let reloads = [0, 1, 0x7FFF, 0xFFFE, 0xFFFF];
        let counters = [0, 1, 0x8000, 0xFFFD, 0xFFFF];
        for enabled in 0u8..16 {
            for cascade in 0u8..16 {
                for irq in 0u8..16 {
                    let mut registers = [Timer::default(); 4];
                    let mut timing = TimerTimingState {
                        clock_phase: u16::from(enabled)
                            | (u16::from(cascade) << 4)
                            | (u16::from(irq) << 8),
                        ..TimerTimingState::default()
                    };
                    timing.clock_phase &= 0x03FF;
                    for index in 0..4 {
                        let period_bits = ((usize::from(enabled)
                            + usize::from(cascade)
                            + usize::from(irq)
                            + index)
                            & 3) as u16;
                        registers[index] = Timer {
                            reload: reloads[(usize::from(cascade) + index) % reloads.len()],
                            counter: counters[(usize::from(irq) + index) % counters.len()],
                            control: period_bits
                                | (u16::from(enabled & (1 << index) != 0) << 7)
                                | (u16::from(cascade & (1 << index) != 0) << 2)
                                | (u16::from(irq & (1 << index) != 0) << 6),
                        };
                        let period = timer_period(registers[index].control);
                        timing.cycle_accum[index] =
                            (u32::from(enabled) * 17 + u32::from(cascade) * 5 + index as u32)
                                & (period - 1);
                        timing.start_delay_cycles[index] = (irq >> index) & 1;
                    }
                    let mut source = Timers::default();
                    source.set_all(registers);
                    assert!(source.set_timing_state(timing));
                    for cycles in [0, 1, 2, 3, 7, 64, 257] {
                        assert_bulk_matches_scalar(source, cycles);
                    }
                }
            }
        }
    }

    #[test]
    fn bulk_step_matches_scalar_for_all_accepted_accumulators() {
        for period_bits in 0..4u16 {
            for accum in 0..=0x03FF {
                for delay in 0..=1 {
                    let mut timers = Timers::default();
                    timers.set_all([
                        Timer {
                            reload: (accum as u16).rotate_left(5),
                            counter: !(accum as u16),
                            control: 0x00C0 | period_bits,
                        },
                        Timer::default(),
                        Timer::default(),
                        Timer::default(),
                    ]);
                    assert!(timers.set_timing_state(TimerTimingState {
                        cycle_accum: [accum, 0, 0, 0],
                        start_delay_cycles: [delay, 0, 0, 0],
                        clock_phase: (accum & 0x03FF) as u16,
                    }));
                    for cycles in [0, 1, 2, 3, 63, 64, 255] {
                        assert_bulk_matches_scalar(timers, cycles);
                    }
                }
            }
        }
    }

    #[test]
    fn bulk_step_matches_scalar_for_deterministic_random_states() {
        let mut random = 0xD1B5_4A32_D192_ED03;
        for _ in 0..2048 {
            let timers = random_timers(&mut random);
            let cycles = next_random(&mut random) & 0x3FFF;
            assert_bulk_matches_scalar(timers, cycles);
        }
    }

    #[test]
    fn random_cycle_partitions_preserve_bulk_results_and_state() {
        let mut random = 0x8A5C_93E7_4B21_60DF;
        for _ in 0..512 {
            let source = random_timers(&mut random);
            let total_cycles = next_random(&mut random) & 0xFFFF;
            let mut whole = source;
            let expected = whole.step_with_overflows(total_cycles);

            let mut partitioned = source;
            let mut flags = 0u16;
            let mut overflows = [0u32; 4];
            let mut remaining = total_cycles;
            while remaining != 0 {
                if next_random(&mut random) & 3 == 0 {
                    assert_eq!(partitioned.step_with_overflows(0), (0, [0; 4], [0; 4]));
                }
                let limit = remaining.min(1024);
                let cycles = 1 + next_random(&mut random) % limit;
                let (next_flags, next_overflows, extra_delays) =
                    partitioned.step_with_overflows(cycles);
                flags |= next_flags;
                for index in 0..4 {
                    overflows[index] = overflows[index].saturating_add(next_overflows[index]);
                }
                assert_eq!(extra_delays, [0; 4]);
                remaining -= cycles;
            }
            assert_eq!((flags, overflows, [0; 4]), expected);
            assert_timer_state_eq(&whole, &partitioned);
        }
    }

    #[test]
    fn tm0_count_up_bit_is_readable_and_uses_cpu_clock_for_all_prescalers() {
        for (prescaler_bits, period) in [(0u16, 1u32), (1, 64), (2, 256), (3, 1024)] {
            let control = 0x00C4 | prescaler_bits;
            let mut timers = Timers::default();
            timers.set_all([
                Timer {
                    reload: 0xFFFC,
                    counter: 0xFFFE,
                    control,
                },
                Timer::default(),
                Timer::default(),
                Timer::default(),
            ]);

            assert_eq!(timers.read16(0, true), control);
            assert!(timers.has_clocked_timers());
            assert_eq!(timers.cycles_until_overflow(0), Some(2 * period));

            let (flags, overflows, extra_delays) = timers.step_with_overflows(period);
            assert_eq!(flags, 0);
            assert_eq!(overflows, [0; 4]);
            assert_eq!(extra_delays, [0; 4]);
            assert_eq!(timers.read16(0, false), 0xFFFF);
            assert_eq!(timers.cycles_until_overflow(0), Some(period));

            let (flags, overflows, extra_delays) = timers.step_with_overflows(period);
            assert_eq!(flags, 1 << 3);
            assert_eq!(overflows, [1, 0, 0, 0]);
            assert_eq!(extra_delays, [0; 4]);
            assert_eq!(timers.read16(0, false), 0xFFFC);
        }
    }

    #[test]
    fn tm0_count_up_bit_overflow_still_drives_irq_and_timer_cascade() {
        let mut timers = Timers::default();
        timers.set_all([
            Timer {
                reload: 0xFFFE,
                counter: 0xFFFF,
                control: 0x00C4,
            },
            Timer {
                reload: 0xFFFD,
                counter: 0xFFFF,
                control: 0x00C4,
            },
            Timer {
                reload: 0,
                counter: 0xFFFE,
                control: 0x0084,
            },
            Timer::default(),
        ]);

        let (flags, overflows, extra_delays) = timers.step_with_overflows(1);

        assert_eq!(flags, (1 << 3) | (1 << 4));
        assert_eq!(overflows, [1, 1, 0, 0]);
        assert_eq!(extra_delays, [0; 4]);
        assert_eq!(
            timers.all().map(|timer| timer.counter),
            [0xFFFE, 0xFFFD, 0xFFFF, 0]
        );
    }

    #[test]
    fn toggling_tm0_count_up_bit_while_enabled_keeps_cpu_clock_and_phase() {
        let mut timers = Timers::default();
        timers.set_all([
            Timer {
                reload: 0,
                counter: 0x1234,
                control: 0x0081,
            },
            Timer::default(),
            Timer::default(),
            Timer::default(),
        ]);
        assert!(timers.set_timing_state(TimerTimingState {
            cycle_accum: [63, 0, 0, 0],
            start_delay_cycles: [0; 4],
            clock_phase: 63,
        }));

        timers.write16(0, true, 0x0085);
        assert_eq!(timers.read16(0, true), 0x0085);
        assert!(timers.has_clocked_timers());
        assert_eq!(timers.step(1), 0);
        assert_eq!(timers.read16(0, false), 0x1235);
        assert_eq!(timers.timing_state().cycle_accum[0], 0);

        timers.write16(0, true, 0x0081);
        assert_eq!(timers.read16(0, true), 0x0081);
        assert_eq!(timers.step(64), 0);
        assert_eq!(timers.read16(0, false), 0x1236);
    }

    #[test]
    fn tm0_count_up_bit_is_cpu_clocked_after_set_all_timing_restore_and_legacy_migration() {
        let timer = Timer {
            reload: 0,
            counter: 0x3456,
            control: 0x0085,
        };
        let mut restored = Timers::default();
        restored.set_all([timer, Timer::default(), Timer::default(), Timer::default()]);
        assert!(restored.has_clocked_timers());
        assert!(restored.set_timing_state(TimerTimingState {
            cycle_accum: [63, 0, 0, 0],
            start_delay_cycles: [0; 4],
            clock_phase: 63,
        }));
        assert_eq!(restored.step(1), 0);
        assert_eq!(restored.read16(0, false), 0x3457);

        let mut legacy = Timers::default();
        legacy.set_all([
            timer,
            Timer {
                control: 0x0084,
                ..Timer::default()
            },
            Timer::default(),
            Timer::default(),
        ]);
        legacy.migrate_legacy_timing(63);
        assert_eq!(legacy.timing_state().cycle_accum, [63, 0, 0, 0]);
        assert_eq!(legacy.step(1), 0);
        assert_eq!(legacy.read16(0, false), 0x3457);
        assert_eq!(legacy.read16(1, false), 0);
    }
    #[test]
    fn maximum_cycle_span_saturates_counts_through_four_timer_cascade() {
        let mut timers = Timers::default();
        timers.set_all([
            Timer {
                reload: 0xFFFF,
                counter: 0xFFFF,
                control: 0x00C0,
            },
            Timer {
                reload: 0xFFFF,
                counter: 0xFFFF,
                control: 0x00C4,
            },
            Timer {
                reload: 0xFFFF,
                counter: 0xFFFF,
                control: 0x00C4,
            },
            Timer {
                reload: 0xFFFF,
                counter: 0xFFFF,
                control: 0x00C4,
            },
        ]);

        let (flags, overflows, extra_delays) = timers.step_with_overflows(u32::MAX);

        assert_eq!(flags, 0x0078);
        assert_eq!(overflows, [u32::MAX; 4]);
        assert_eq!(extra_delays, [0; 4]);
        assert_eq!(timers.all().map(|timer| timer.counter), [0xFFFF; 4]);
        assert_eq!(timers.timing_state().clock_phase, 0x03FF);
    }

    #[test]
    fn maximum_cycle_span_preserves_scalar_accumulator_saturation() {
        let mut timers = Timers::default();
        timers.set_all([
            Timer {
                reload: 0,
                counter: 0,
                control: 0x0083,
            },
            Timer::default(),
            Timer::default(),
            Timer::default(),
        ]);
        assert!(timers.set_timing_state(TimerTimingState {
            cycle_accum: [0x03FF, 0, 0, 0],
            start_delay_cycles: [0; 4],
            clock_phase: 0,
        }));

        let (_, overflows, _) = timers.step_with_overflows(u32::MAX);

        assert_eq!(overflows, [63, 0, 0, 0]);
        assert_eq!(timers.read16(0, false), 0xFFFF);
        assert_eq!(timers.timing_state().cycle_accum[0], 0x03FF);
    }
}

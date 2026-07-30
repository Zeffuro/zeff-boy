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
    enable_delay_pending: [bool; 4],
    enable_delay_cycles: [u32; 4],
    // Runtime-only scheduler phase for the first overflow after enabling a timer.
    //
    // GBATEK documents TMCNT_L as a reload register: writes affect the next
    // start/overflow, not the already-running counter. mGBA models this with
    // scheduled timer events plus a delayed IRQ event. This interpreter still
    // applies IO writes at instruction boundaries, so this keeps first-overflow
    // IRQ phasing close until timer IO becomes fully cycle-addressed.
    first_overflow_irq_extra_delay: [u32; 4],
    first_overflow_low_read_seen: [bool; 4],
}

pub type TimerOverflowCounts = [u32; 4];
pub type TimerIrqExtraDelays = [u32; 4];
pub type TimerIrqCyclesLate = [u32; 4];

const FIRST_OVERFLOW_IRQ_EXTRA_DELAY: u32 = 1;
const FIRST_OVERFLOW_IRQ_EXTRA_DELAY_AFTER_LOW_READ: u32 = 3;
const ACTIVE_RELOAD_WRITE_FIRST_OVERFLOW_IRQ_EXTRA_DELAY: u32 = 1;
const TIMER_LOW_READ_LATE_CYCLES: u32 = 0;
const TIMER_DISABLE_WRITE_SYNC_CYCLES: u32 = 0;

impl Timers {
    pub fn read16(&self, index: usize, control: bool) -> u16 {
        let timer = self.timers.get(index).copied().unwrap_or_default();
        if control {
            timer.control
        } else {
            timer.counter
        }
    }

    pub fn cpu_read16(&mut self, index: usize, control: bool) -> u16 {
        if let Some(timer) = self.timers.get(index)
            && !control
            && timer.control & 0x0080 != 0
            && self.first_overflow_irq_extra_delay[index] != 0
        {
            self.first_overflow_low_read_seen[index] = true;
        }
        if control {
            self.read16(index, true)
        } else {
            self.project_counter(index, TIMER_LOW_READ_LATE_CYCLES)
                .unwrap_or_else(|| self.read16(index, false))
        }
    }

    pub fn write16(&mut self, index: usize, control: bool, value: u16) {
        let disable_active_timer = control
            && self
                .timers
                .get(index)
                .is_some_and(|timer| timer.control & 0x0080 != 0 && value & 0x0080 == 0);
        if disable_active_timer {
            self.sync_counter_without_events(index, TIMER_DISABLE_WRITE_SYNC_CYCLES);
        }
        if let Some(timer) = self.timers.get_mut(index) {
            if control {
                let old_control = timer.control;
                timer.control = value & 0x00C7;
                if old_control & 0x0080 == 0 && timer.control & 0x0080 != 0 {
                    timer.counter = timer.reload;
                    if let Some(accum) = self.cycle_accum.get_mut(index) {
                        *accum = 0;
                    }
                    self.enable_delay_pending[index] = true;
                    self.enable_delay_cycles[index] = 0;
                    self.first_overflow_irq_extra_delay[index] = if timer.reload == 0xFFFF {
                        0
                    } else {
                        FIRST_OVERFLOW_IRQ_EXTRA_DELAY
                    };
                    self.first_overflow_low_read_seen[index] = false;
                } else if timer.control & 0x0080 == 0 {
                    self.first_overflow_irq_extra_delay[index] = 0;
                    self.first_overflow_low_read_seen[index] = false;
                }
            } else {
                timer.reload = value;
                if timer.control & 0x0080 != 0 && self.first_overflow_irq_extra_delay[index] != 0 {
                    self.first_overflow_irq_extra_delay[index] =
                        ACTIVE_RELOAD_WRITE_FIRST_OVERFLOW_IRQ_EXTRA_DELAY;
                }
            }
        }
    }

    pub fn begin_step_window(&mut self, cycles: u32) {
        for index in 0..self.timers.len() {
            if self.enable_delay_pending[index] {
                let immediate_overflow_delay = u32::from(self.timers[index].counter == 0xFFFF);
                self.enable_delay_cycles[index] = self.enable_delay_cycles[index]
                    .saturating_add(cycles)
                    .saturating_add(immediate_overflow_delay);
                self.enable_delay_pending[index] = false;
            }
        }
    }

    pub fn step(&mut self, cycles: u32) -> u16 {
        self.step_with_overflows(cycles).0
    }

    pub fn step_with_overflows(
        &mut self,
        cycles: u32,
    ) -> (
        u16,
        TimerOverflowCounts,
        TimerIrqExtraDelays,
        TimerIrqCyclesLate,
    ) {
        let mut irq_flags = 0u16;
        let mut overflow_counts = [0u32; 4];
        let mut irq_extra_delays = [0u32; 4];
        let mut irq_cycles_late = [0u32; 4];
        for index in 0..4 {
            let timer = self.timers[index];
            if timer.control & 0x0080 == 0 || timer.control & 0x0004 != 0 {
                continue;
            }

            let count_cycles = self.consume_enable_delay(index, cycles);
            if count_cycles == 0 {
                continue;
            }

            let count_start_offset = cycles - count_cycles;
            self.cycle_accum[index] = self.cycle_accum[index].saturating_add(count_cycles);
            let period = timer_period(timer.control);
            let mut event_offset = count_start_offset + period.saturating_sub(
                self.cycle_accum[index]
                    .saturating_sub(count_cycles),
            );
            while self.cycle_accum[index] >= period {
                self.cycle_accum[index] -= period;
                let cycles_late = cycles.saturating_sub(event_offset);
                if self.increment_timer(
                    index,
                    &mut irq_flags,
                    &mut overflow_counts,
                    &mut irq_extra_delays,
                    &mut irq_cycles_late,
                    cycles_late,
                ) {
                    self.increment_cascade(
                        index + 1,
                        &mut irq_flags,
                        &mut overflow_counts,
                        &mut irq_extra_delays,
                        &mut irq_cycles_late,
                        cycles_late,
                    );
                }
                event_offset = event_offset.saturating_add(period);
            }
        }
        (
            irq_flags,
            overflow_counts,
            irq_extra_delays,
            irq_cycles_late,
        )
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
        let enable_delay = self.enable_delay_cycles.get(index).copied().unwrap_or(0);
        let cycles_until_increment = period.saturating_sub(accum).max(1);
        let increments_until_overflow = 0x1_0000 - u32::from(timer.counter);
        Some(
            enable_delay
                .saturating_add(cycles_until_increment)
                .saturating_add(
                    increments_until_overflow
                        .saturating_sub(1)
                        .saturating_mul(period),
                ),
        )
    }

    pub fn set_all(&mut self, timers: [Timer; 4]) {
        self.timers = timers;
        self.cycle_accum = [0; 4];
        self.enable_delay_pending = [false; 4];
        self.enable_delay_cycles = [0; 4];
        self.first_overflow_irq_extra_delay = [0; 4];
        self.first_overflow_low_read_seen = [false; 4];
    }

    fn increment_cascade(
        &mut self,
        index: usize,
        irq_flags: &mut u16,
        overflow_counts: &mut TimerOverflowCounts,
        irq_extra_delays: &mut TimerIrqExtraDelays,
        irq_cycles_late: &mut TimerIrqCyclesLate,
        cycles_late: u32,
    ) {
        if index >= self.timers.len() {
            return;
        }
        let timer = self.timers[index];
        if timer.control & 0x0080 == 0 || timer.control & 0x0004 == 0 {
            return;
        }
        if self.increment_timer(
            index,
            irq_flags,
            overflow_counts,
            irq_extra_delays,
            irq_cycles_late,
            cycles_late,
        ) {
            self.increment_cascade(
                index + 1,
                irq_flags,
                overflow_counts,
                irq_extra_delays,
                irq_cycles_late,
                cycles_late,
            );
        }
    }

    fn increment_timer(
        &mut self,
        index: usize,
        irq_flags: &mut u16,
        overflow_counts: &mut TimerOverflowCounts,
        irq_extra_delays: &mut TimerIrqExtraDelays,
        irq_cycles_late: &mut TimerIrqCyclesLate,
        cycles_late: u32,
    ) -> bool {
        let timer = &mut self.timers[index];
        let (counter, overflowed) = timer.counter.overflowing_add(1);
        if overflowed {
            timer.counter = timer.reload;
            overflow_counts[index] = overflow_counts[index].saturating_add(1);
            if timer.control & 0x0040 != 0 {
                *irq_flags |= 1 << (3 + index);
                irq_extra_delays[index] = if self.first_overflow_low_read_seen[index] {
                    self.first_overflow_irq_extra_delay[index]
                        .max(FIRST_OVERFLOW_IRQ_EXTRA_DELAY_AFTER_LOW_READ)
                } else {
                    self.first_overflow_irq_extra_delay[index]
                };
                irq_cycles_late[index] = irq_cycles_late[index].max(cycles_late);
            }
            self.first_overflow_irq_extra_delay[index] = 0;
            self.first_overflow_low_read_seen[index] = false;
            true
        } else {
            timer.counter = counter;
            false
        }
    }

    fn consume_enable_delay(&mut self, index: usize, cycles: u32) -> u32 {
        let delay = self.enable_delay_cycles[index].min(cycles);
        self.enable_delay_cycles[index] -= delay;
        cycles - delay
    }

    fn project_counter(&self, index: usize, cycles: u32) -> Option<u16> {
        let timer = self.timers.get(index).copied()?;
        if timer.control & 0x0080 == 0 || timer.control & 0x0004 != 0 {
            return Some(timer.counter);
        }

        let count_cycles = cycles.saturating_sub(
            self.enable_delay_cycles
                .get(index)
                .copied()
                .unwrap_or(0)
                .min(cycles),
        );
        let period = timer_period(timer.control);
        let increments = (self.cycle_accum.get(index).copied().unwrap_or(0) + count_cycles) / period;
        Some(project_counter_increments(
            timer.counter,
            timer.reload,
            increments,
        ))
    }

    fn sync_counter_without_events(&mut self, index: usize, cycles: u32) {
        let Some(timer) = self.timers.get(index).copied() else {
            return;
        };
        if timer.control & 0x0080 == 0 || timer.control & 0x0004 != 0 {
            return;
        }

        let delay = self.enable_delay_cycles[index].min(cycles);
        let count_cycles = cycles - delay;
        self.enable_delay_cycles[index] -= delay;
        self.cycle_accum[index] = self.cycle_accum[index].saturating_add(count_cycles);
        let period = timer_period(timer.control);
        let increments = self.cycle_accum[index] / period;
        self.cycle_accum[index] %= period;
        self.timers[index].counter =
            project_counter_increments(timer.counter, timer.reload, increments);
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

fn project_counter_increments(counter: u16, reload: u16, increments: u32) -> u16 {
    if increments == 0 {
        return counter;
    }

    let until_overflow = 0x1_0000 - u32::from(counter);
    if increments < until_overflow {
        return counter.wrapping_add(increments as u16);
    }

    let cycle_len = 0x1_0000 - u32::from(reload);
    let after_first_overflow = increments - until_overflow;
    reload.wrapping_add((after_first_overflow % cycle_len) as u16)
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
        timers.begin_step_window(1);

        let flags = timers.step(1);

        assert_eq!(flags, 0);
        assert_eq!(timers.read16(0, false), 0xFFFF);

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
        timers.begin_step_window(1);

        timers.step(1);
        assert_eq!(timers.read16(1, false), 0xFFFE);

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

        timers.begin_step_window(1);
        let (flags, overflows, _, _) = timers.step_with_overflows(1);
        assert_eq!(flags, 0);
        assert_eq!(overflows[0], 0);

        let (flags, overflows, _, _) = timers.step_with_overflows(1);
        assert_eq!(flags, 0);
        assert_eq!(overflows[0], 0);

        let (flags, overflows, _, _) = timers.step_with_overflows(4);

        assert_eq!(flags, 0);
        assert_eq!(overflows[0], 4);
        assert_eq!(timers.read16(0, false), 0xFFFF);
    }

    #[test]
    fn cycles_until_overflow_accounts_for_prescaler_and_counter() {
        let mut timers = Timers::default();
        timers.write16(0, false, 0xFFFE);
        timers.write16(0, true, 0x0081);
        timers.begin_step_window(1);

        assert_eq!(timers.cycles_until_overflow(0), Some(129));

        timers.step(32);
        assert_eq!(timers.cycles_until_overflow(0), Some(97));

        timers.step(97);
        assert_eq!(timers.read16(0, false), 0xFFFE);
        assert_eq!(timers.cycles_until_overflow(0), Some(128));
    }

    #[test]
    fn newly_enabled_timer_does_not_count_current_step_window() {
        let mut timers = Timers::default();
        timers.write16(0, false, 0xFFFF);
        timers.write16(0, true, 0x0080);

        timers.begin_step_window(3);
        let (_, overflows, _, _) = timers.step_with_overflows(3);
        assert_eq!(overflows[0], 0);
        assert_eq!(timers.read16(0, false), 0xFFFF);

        let (_, overflows, _, _) = timers.step_with_overflows(1);
        assert_eq!(overflows[0], 0);

        let (_, overflows, _, _) = timers.step_with_overflows(1);
        assert_eq!(overflows[0], 1);
    }

    #[test]
    fn timer_irq_reports_cycles_late_for_overflow_inside_step() {
        let mut timers = Timers::default();
        timers.set_all([
            Timer {
                reload: 0xFFFF,
                counter: 0xFFFF,
                control: 0x00C0,
            },
            Timer::default(),
            Timer::default(),
            Timer::default(),
        ]);

        let (flags, overflows, _, cycles_late) = timers.step_with_overflows(3);

        assert_eq!(flags, 1 << 3);
        assert_eq!(overflows[0], 3);
        assert_eq!(cycles_late[0], 2);
    }
}

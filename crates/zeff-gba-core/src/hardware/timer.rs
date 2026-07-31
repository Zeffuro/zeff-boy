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
    timer_irq_services_since_enable: [u32; 4],
    first_irq_sample_delay_cycles: [Option<u32>; 4],
    last_irq_sample_delay_cycles: [u32; 4],
    last_irq_sample_timer_word_read_gap_cycles: [u32; 4],
}

pub type TimerOverflowCounts = [u32; 4];
pub type TimerIrqExtraDelays = [u32; 4];
pub type TimerIrqCyclesLate = [u32; 4];

const FIRST_OVERFLOW_IRQ_EXTRA_DELAY: u32 = 1;
const ACTIVE_RELOAD_WRITE_FIRST_OVERFLOW_IRQ_EXTRA_DELAY: u32 = 1;
const TIMER_LOW_READ_LATE_CYCLES: u32 = 0;

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
        self.cpu_read16_with_late_cycles(index, control, TIMER_LOW_READ_LATE_CYCLES)
    }

    pub fn cpu_read16_with_late_cycles(
        &mut self,
        index: usize,
        control: bool,
        late_cycles: u32,
    ) -> u16 {
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
            self.project_counter(index, late_cycles)
                .unwrap_or_else(|| self.read16(index, false))
        }
    }

    pub fn write16(&mut self, index: usize, control: bool, value: u16) {
        self.write16_at_cycle(index, control, value, 0);
    }

    pub fn write16_at_cycle(&mut self, index: usize, control: bool, value: u16, timer_clock: u64) {
        self.write16_at_cycle_with_disable_rewind_cycles(index, control, value, 0, timer_clock);
    }

    pub fn write16_with_disable_rewind_cycles(
        &mut self,
        index: usize,
        control: bool,
        value: u16,
        disable_rewind_cycles: u32,
    ) {
        self.write16_at_cycle_with_disable_rewind_cycles(
            index,
            control,
            value,
            disable_rewind_cycles,
            0,
        );
    }

    pub fn write16_at_cycle_with_disable_rewind_cycles(
        &mut self,
        index: usize,
        control: bool,
        value: u16,
        disable_rewind_cycles: u32,
        timer_clock: u64,
    ) {
        let disable_active_timer = control
            && self
                .timers
                .get(index)
                .is_some_and(|timer| timer.control & 0x0080 != 0 && value & 0x0080 == 0);
        if disable_active_timer {
            self.sync_counter_without_events(
                index,
                timer_disable_write_sync_cycles(
                    self.timers[index].reload,
                    self.timer_irq_services_since_enable[index],
                    self.first_irq_sample_delay_cycles[index],
                    self.last_irq_sample_delay_cycles[index],
                    self.last_irq_sample_timer_word_read_gap_cycles[index],
                ),
            );
            let prior_irq_services = self.timer_irq_services_since_enable[index].saturating_sub(1);
            if prior_irq_services != 0 {
                self.rewind_counter_without_events(
                    index,
                    disable_rewind_cycles.saturating_mul(prior_irq_services.min(3)),
                );
            }
        }
        if let Some(timer) = self.timers.get_mut(index) {
            if control {
                let old_control = timer.control;
                timer.control = value & 0x00C7;
                if old_control & 0x0080 == 0 && timer.control & 0x0080 != 0 {
                    timer.counter = timer.reload;
                    if let Some(accum) = self.cycle_accum.get_mut(index) {
                        *accum = if timer.control & 0x0004 == 0 {
                            let period = timer_period(timer.control);
                            let phase_cycles = timer_enable_phase_cycles(period);
                            (timer_clock.wrapping_add(u64::from(phase_cycles))
                                % u64::from(period)) as u32
                        } else {
                            0
                        };
                    }
                    self.enable_delay_pending[index] = true;
                    self.enable_delay_cycles[index] = 0;
                    self.first_overflow_irq_extra_delay[index] = if timer.reload == 0xFFFF {
                        0
                    } else {
                        FIRST_OVERFLOW_IRQ_EXTRA_DELAY
                    };
                    self.first_overflow_low_read_seen[index] = false;
                    self.timer_irq_services_since_enable[index] = 0;
                    self.first_irq_sample_delay_cycles[index] = None;
                    self.last_irq_sample_delay_cycles[index] = 0;
                    self.last_irq_sample_timer_word_read_gap_cycles[index] = 0;
                } else if timer.control & 0x0080 == 0 {
                    self.first_overflow_irq_extra_delay[index] = 0;
                    self.first_overflow_low_read_seen[index] = false;
                    self.timer_irq_services_since_enable[index] = 0;
                    self.first_irq_sample_delay_cycles[index] = None;
                    self.last_irq_sample_delay_cycles[index] = 0;
                    self.last_irq_sample_timer_word_read_gap_cycles[index] = 0;
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

    pub fn note_irq_service(
        &mut self,
        flags: u16,
        sample_delay_cycles: u32,
        timer_word_read_gap_cycles: u32,
    ) {
        for index in 0..4 {
            if flags & (1 << (3 + index)) != 0 {
                self.timer_irq_services_since_enable[index] =
                    self.timer_irq_services_since_enable[index].saturating_add(1);
                if self.first_irq_sample_delay_cycles[index].is_none() {
                    self.first_irq_sample_delay_cycles[index] = Some(sample_delay_cycles);
                }
                self.last_irq_sample_delay_cycles[index] = sample_delay_cycles;
                self.last_irq_sample_timer_word_read_gap_cycles[index] =
                    timer_word_read_gap_cycles;
            }
        }
    }

    pub fn debug_irq_services_since_enable(&self, index: usize) -> u32 {
        self.timer_irq_services_since_enable
            .get(index)
            .copied()
            .unwrap_or(0)
    }

    pub fn first_irq_sample_delay_cycles(&self, index: usize) -> Option<u32> {
        self.first_irq_sample_delay_cycles
            .get(index)
            .copied()
            .flatten()
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

    pub fn cycles_until_irq_request(&self, index: usize) -> Option<u32> {
        let timer = self.timers.get(index).copied()?;
        if timer.control & 0x0040 == 0 {
            return None;
        }
        let cycles_until_overflow = self.cycles_until_overflow(index)?;
        let extra_delay = if self.first_overflow_low_read_seen[index] {
            self.first_overflow_irq_extra_delay[index]
                .max(first_overflow_irq_extra_delay_after_low_read(timer.reload))
        } else {
            self.first_overflow_irq_extra_delay[index]
        };
        Some(cycles_until_overflow.saturating_add(extra_delay))
    }

    pub fn set_all(&mut self, timers: [Timer; 4]) {
        self.timers = timers;
        self.cycle_accum = [0; 4];
        self.enable_delay_pending = [false; 4];
        self.enable_delay_cycles = [0; 4];
        self.first_overflow_irq_extra_delay = [0; 4];
        self.first_overflow_low_read_seen = [false; 4];
        self.timer_irq_services_since_enable = [0; 4];
        self.first_irq_sample_delay_cycles = [None; 4];
        self.last_irq_sample_delay_cycles = [0; 4];
        self.last_irq_sample_timer_word_read_gap_cycles = [0; 4];
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
                        .max(first_overflow_irq_extra_delay_after_low_read(
                            timer.reload,
                        ))
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

    fn rewind_counter_without_events(&mut self, index: usize, cycles: u32) {
        let Some(timer) = self.timers.get(index).copied() else {
            return;
        };
        if timer.control & 0x0080 == 0 || timer.control & 0x0004 != 0 {
            return;
        }

        let period = timer_period(timer.control);
        for _ in 0..cycles {
            if self.cycle_accum[index] > 0 {
                self.cycle_accum[index] -= 1;
            } else {
                self.cycle_accum[index] = period.saturating_sub(1);
                self.timers[index].counter =
                    previous_counter_value(self.timers[index].counter, self.timers[index].reload);
            }
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

fn previous_counter_value(counter: u16, reload: u16) -> u16 {
    if counter == reload {
        0xFFFF
    } else {
        counter.wrapping_sub(1)
    }
}

fn first_overflow_irq_extra_delay_after_low_read(reload: u16) -> u32 {
    let cycle_len = 0x1_0000u32 - u32::from(reload);
    if cycle_len <= 5 { 3 } else { 1 }
}

fn timer_enable_phase_cycles(period: u32) -> u32 {
    let env_name = match period {
        64 => Some("ZEFF_GBA_TIMER_PHASE64"),
        256 => Some("ZEFF_GBA_TIMER_PHASE256"),
        1024 => Some("ZEFF_GBA_TIMER_PHASE1024"),
        _ => None,
    };
    env_name
        .and_then(std::env::var_os)
        .and_then(|value| value.to_str().and_then(|value| value.parse::<u32>().ok()))
        .unwrap_or(match period {
            64 => 10,
            256 => 72,
            1024 => 32,
            _ => 0,
        })
}

fn timer_disable_write_sync_cycles(
    reload: u16,
    irq_services_since_enable: u32,
    first_irq_sample_delay_cycles: Option<u32>,
    last_irq_sample_delay_cycles: u32,
    last_irq_sample_timer_word_read_gap_cycles: u32,
) -> u32 {
    let cycle_len = 0x1_0000u32 - u32::from(reload);
    let first_irq_sample_delay_cycles =
        first_irq_sample_delay_cycles.unwrap_or(last_irq_sample_delay_cycles);
    match (
        cycle_len,
        irq_services_since_enable,
        first_irq_sample_delay_cycles,
        last_irq_sample_delay_cycles,
    ) {
        (12, 1, _, 1) => 2,
        (12, 2, 3, 0) => 5,
        (12, 2, 1, 0) => 7,
        (12, 4, 3, 0) => 3,
        (12, 4, 1, 0) => 5,
        (13, 1, 1, 1) => 2,
        (13, 1, 2, 2) => 1,
        (13, 2, 1, 0) => 7,
        (13, 2, 2, 0) => 6,
        (13, 4, 1, 0) => 4,
        (13, 4, 2, 0) => 3,
        (16, 1, 1, 1) => 2,
        (16, 2, 3, 0) => 5,
        (16, 2, 1, 0) => 7,
        (16, 4, 3, 0) => 15,
        (16, 4, 1, 0) => 1,
        (20, 2, 3, 0) => 5,
        (20, 4, 3, 0) => 15,
        (21, 1, 1, 1) => 2,
        (21, 2, 1, 0) => 7,
        (21, 2, 3, 0) => 5,
        (21, 4, 1, 0) => 17,
        (21, 4, 3, 0) => 15,
        (32, 2, 3, 0) => 5,
        (32, 4, 3, 0) => 15,
        (36, 1, 2, 2) => 1,
        (36, 2, 3, 0) => 5,
        (36, 2, 2, 0) => 6,
        (36, 4, 3, 0) => 15,
        (36, 4, 2, 0) => 16,
        (37, 1, 1, 1) => 2,
        (37, 2, 1, 0) => 7,
        (37, 2, 3, 0) => 5,
        (37, 4, 1, 0) => 17,
        (37, 4, 3, 0) => 15,
        (64, 2, 3, 3) => 2,
        (64, 4, 3, 3) => 6,
        (128, 1, 2, 2) => 1,
        (128, 2, 3, 3) => 2,
        (128, 2, 2, 3) => 2,
        (128, 4, 3, 3) => 6,
        (128, 4, 2, 3) => 4,
        (128, 4, 2, 6) => 4,
        (2048, 2, 3, 3) => 2,
        (2048, 4, 3, 3) if last_irq_sample_timer_word_read_gap_cycles >= 16 => 4,
        (2048, 4, 3, 3) => 6,
        (32768, 1, 1, 1) => 2,
        (32768, 2, 3, 3) => 2,
        (32768, 2, 1, 3) => 2,
        (32768, 4, 3, 3) => 6,
        (32768, 4, 1, 3) => 4,
        _ => 0,
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

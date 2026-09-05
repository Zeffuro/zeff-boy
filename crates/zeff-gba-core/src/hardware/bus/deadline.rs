use super::{Bus, IE, IF, read_io16};

const IRQ_DELAY_CYCLES: u32 = 7;
const IRQ_SAMPLE_LOOKAHEAD_CYCLES: u32 = 3;
pub(super) const DEADLINE_PPU: u8 = 1 << 0;
pub(super) const DEADLINE_IRQ: u8 = 1 << 1;
pub(super) const DEADLINE_TIMER_SHIFT: u8 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct BusDeadline {
    pub(super) remaining: u32,
    pub(super) sources: u8,
}

impl Bus {
    #[cold]
    #[inline(never)]
    pub(super) fn fresh_event_deadline(&self) -> BusDeadline {
        let mut deadline = BusDeadline {
            remaining: self.ppu.cycles_until_next_status_event().max(1),
            sources: DEADLINE_PPU,
        };
        merge_deadline(&mut deadline, self.cycles_until_irq_event(), DEADLINE_IRQ);
        if self.timers.has_clocked_timers() {
            for timer in 0..4 {
                if let Some(next) = self.timer_cycles_until_overflow(timer) {
                    merge_deadline(
                        &mut deadline,
                        next.max(1),
                        1 << (DEADLINE_TIMER_SHIFT + timer as u8),
                    );
                }
            }
        }
        deadline
    }

    pub(super) fn invalidate_event_deadline(&mut self, reason: usize) {
        self.event_deadline = BusDeadline::default();
        #[cfg(feature = "profiling")]
        {
            self.profiling.deadline_invalidations[reason] =
                self.profiling.deadline_invalidations[reason].wrapping_add(1);
        }
        #[cfg(not(feature = "profiling"))]
        let _ = reason;
    }

    pub(crate) fn invalidate_event_deadline_after_state_load(&mut self) {
        self.invalidate_event_deadline(2);
    }

    #[cfg(test)]
    pub(crate) fn event_deadline_is_invalid_for_test(&self) -> bool {
        self.event_deadline.remaining == 0
    }

    pub fn cycles_until_next_halt_check(&self) -> u32 {
        let mut cycles = 64;
        cycles = cycles.min(self.ppu.cycles_until_next_status_event().max(1));
        cycles = cycles.min(self.cycles_until_irq_event());
        if self.timers.has_clocked_timers() {
            for timer in 0..4 {
                if let Some(next) = self.timer_cycles_until_overflow(timer) {
                    cycles = cycles.min(next.max(1));
                }
            }
        }
        cycles.max(1)
    }

    pub(crate) fn interrupt_ready(&self) -> bool {
        self.interrupt_pending()
            && self
                .irq_delay_cycles
                .is_some_and(|cycles| cycles <= IRQ_SAMPLE_LOOKAHEAD_CYCLES)
    }

    pub(crate) fn take_irq_sample_delay_cycles(&mut self) -> u32 {
        let Some(cycles) = self.irq_delay_cycles else {
            return 0;
        };
        if cycles > IRQ_SAMPLE_LOOKAHEAD_CYCLES {
            return 0;
        }
        self.irq_delay_cycles = Some(0);
        self.invalidate_event_deadline(1);
        cycles
    }

    pub(crate) fn irq_delay_state(&self) -> Option<u32> {
        self.irq_delay_cycles
    }

    pub(crate) fn set_irq_delay_state(&mut self, delay: Option<u32>) -> bool {
        if delay.is_some_and(|delay| delay > IRQ_DELAY_CYCLES) {
            return false;
        }
        self.irq_delay_cycles = delay;
        self.invalidate_event_deadline(1);
        true
    }

    pub(crate) fn migrate_legacy_irq_delay(&mut self) {
        self.irq_delay_cycles = self.irq_line_asserted().then_some(IRQ_DELAY_CYCLES);
        self.invalidate_event_deadline(1);
    }

    pub(crate) fn test_irq_signal(&mut self, cycles_late: u32) {
        self.test_irq_signal_with_extra_delay(cycles_late, 0);
    }

    pub(crate) fn test_irq_signal_with_extra_delay(&mut self, cycles_late: u32, extra_delay: u32) {
        if self.irq_line_asserted() && self.irq_delay_cycles.is_none() {
            self.irq_delay_cycles = Some(
                IRQ_DELAY_CYCLES
                    .saturating_add(extra_delay)
                    .saturating_sub(cycles_late),
            );
            self.invalidate_event_deadline(1);
        }
    }

    pub(super) fn cycles_until_irq_event(&self) -> u32 {
        self.irq_delay_cycles
            .filter(|&cycles| cycles > 0)
            .unwrap_or(u32::MAX)
    }

    pub(super) fn step_irq_event(&mut self, cycles: u32) {
        if let Some(delay) = self.irq_delay_cycles {
            let next = delay.saturating_sub(cycles);
            self.irq_delay_cycles = if next == 0 && !self.irq_line_asserted() {
                None
            } else {
                Some(next)
            };
        }
    }

    fn irq_line_asserted(&self) -> bool {
        read_io16(&self.io, IE) & read_io16(&self.io, IF) & 0x3FFF != 0
    }
}

fn merge_deadline(deadline: &mut BusDeadline, candidate: u32, source: u8) {
    match candidate.cmp(&deadline.remaining) {
        std::cmp::Ordering::Less => {
            deadline.remaining = candidate;
            deadline.sources = source;
        }
        std::cmp::Ordering::Equal => deadline.sources |= source,
        std::cmp::Ordering::Greater => {}
    }
}

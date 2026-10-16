use super::super::chip_constants::{TIMER_B_STEPS, TIMER_B_SUBCOUNTER_STEPS};
use super::super::scheduler::SLOTS;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TimerB {
    pub(crate) reload: u8,
    counter: u8,
    subcounter: u8,
    load_lock: bool,
    reload_pending: bool,
    overflow: bool,
    clear: bool,
}

impl TimerB {
    pub(crate) fn clear(&mut self, value: bool) {
        self.clear = value;
    }

    pub(crate) fn clock(&mut self, cycle: usize, mode: u8, flags: &mut u8) {
        if cycle != 1 && cycle != 2 && !self.reload_pending && !self.overflow && !self.clear {
            return;
        }
        let mut load = self.overflow;
        if cycle == 2 {
            load |= mode & 2 != 0 && !self.load_lock;
            self.load_lock = mode & 2 != 0;
        }
        let mut value = u16::from(if self.reload_pending {
            self.reload
        } else {
            self.counter
        });
        self.reload_pending = load;
        if cycle == 1 {
            self.subcounter += 1;
        }
        if self.subcounter == TIMER_B_SUBCOUNTER_STEPS && self.load_lock {
            value += 1;
        }
        self.subcounter &= TIMER_B_SUBCOUNTER_STEPS - 1;
        if self.clear {
            *flags &= !2;
            self.clear = false;
        } else if self.overflow && mode & 8 != 0 {
            *flags |= 2;
        }
        self.overflow = value >= TIMER_B_STEPS;
        self.counter = value as u8;
    }

    pub(crate) fn advance_slots(
        &mut self,
        mut cycle: usize,
        mut slots: u64,
        mode: u8,
        flags: &mut u8,
    ) {
        let request = mode & 2 != 0;
        while slots != 0
            && (cycle != 4
                || self.load_lock != request
                || self.reload_pending
                || self.overflow
                || self.clear)
        {
            self.clock(cycle, mode, flags);
            cycle = (cycle + 1) % SLOTS;
            slots -= 1;
        }
        let scans = slots / SLOTS as u64;
        let divider = u64::from(TIMER_B_SUBCOUNTER_STEPS);
        let tail = scans % divider + u64::from(self.subcounter);
        let pulses = scans / divider + tail / divider;
        self.subcounter = (tail % divider) as u8;
        if request && pulses != 0 {
            let first = u64::from(TIMER_B_STEPS - u16::from(self.counter));
            if pulses < first {
                self.counter += pulses as u8;
            } else {
                let period = u64::from(TIMER_B_STEPS - u16::from(self.reload));
                self.counter = self.reload + ((pulses - first) % period) as u8;
                if mode & 8 != 0 {
                    *flags |= 2;
                }
            }
        }
        for _ in 0..slots % SLOTS as u64 {
            self.clock(cycle, mode, flags);
            cycle = (cycle + 1) % SLOTS;
        }
    }
}

mod state;
#[cfg(test)]
mod tests;

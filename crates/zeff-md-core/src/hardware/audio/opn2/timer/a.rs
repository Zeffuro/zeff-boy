use super::super::chip_constants::{FM_CLOCK_DIVIDER, TIMER_A_STEPS};
use super::super::scheduler::SLOTS;
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TimerA {
    pub(crate) reload: u16,
    counter: u16,
    load_lock: bool,
    reload_pending: bool,
    overflow: bool,
    clear: bool,
    csm: bool,
}

impl TimerA {
    pub(crate) fn clear(&mut self, value: bool) {
        self.clear = value;
    }

    pub(crate) fn csm(&self) -> bool {
        self.csm
    }

    pub(crate) fn can_key(&self, mode: u8) -> bool {
        self.csm
            || (mode & 0xc0 == 0x80
                && (mode & 1 != 0 || self.load_lock || self.reload_pending || self.overflow))
    }

    pub(crate) fn clock(&mut self, cycle: usize, mode: u8, flags: &mut u8) {
        let mut load = self.overflow;
        if cycle == 2 {
            load |= mode & 1 != 0 && !self.load_lock;
            self.load_lock = mode & 1 != 0;
            self.csm = mode & 0xc0 == 0x80 && load;
        }
        let mut value = if self.reload_pending {
            self.reload
        } else {
            self.counter
        };
        self.reload_pending = load;
        if cycle == 1 && self.load_lock {
            value += 1;
        }
        if self.clear {
            *flags &= !1;
            self.clear = false;
        } else if self.overflow && mode & 4 != 0 {
            *flags |= 1;
        }
        self.overflow = value >= TIMER_A_STEPS;
        self.counter = value & (TIMER_A_STEPS - 1);
    }

    pub(crate) fn advance_slots(
        &mut self,
        mut cycle: usize,
        mut slots: u64,
        mode: u8,
        flags: &mut u8,
    ) {
        let request = mode & 1 != 0;
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
        let frames = slots / SLOTS as u64;
        if frames != 0 {
            self.csm = false;
            if request {
                let first = u64::from(TIMER_A_STEPS - self.counter);
                if frames < first {
                    self.counter += frames as u16;
                } else {
                    let period = u64::from(TIMER_A_STEPS - self.reload);
                    let tail = (frames - first) % period;
                    self.counter = self.reload + tail as u16;
                    self.csm = mode & 0xc0 == 0x80 && tail == 0;
                    if mode & 4 != 0 {
                        *flags |= 1;
                    }
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

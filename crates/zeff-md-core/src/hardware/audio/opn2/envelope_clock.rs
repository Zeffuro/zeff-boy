use super::constants::{ENVELOPE_COUNTER_MAX, ENVELOPE_DIVIDER, ENVELOPE_PATTERNS};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct EnvelopeClock {
    pub(super) counter: u16,
    pub(super) phase: u8,
}

impl EnvelopeClock {
    pub(super) fn clock(&mut self) -> Option<u16> {
        self.phase += 1;
        if self.phase != ENVELOPE_DIVIDER {
            return None;
        }
        self.phase = 0;
        // Timer overflow is consumed as an end-around carry before the next EG beat.
        self.counter = self.counter % ENVELOPE_COUNTER_MAX + 1;
        Some(self.counter)
    }

    pub(super) fn advance(&mut self, clocks: u64) {
        let cycles = clocks / u64::from(ENVELOPE_DIVIDER);
        let remainder = clocks % u64::from(ENVELOPE_DIVIDER) + u64::from(self.phase);
        let steps = cycles + remainder / u64::from(ENVELOPE_DIVIDER);
        if steps != 0 {
            self.counter = ((u64::from(self.counter) + steps - 1) % u64::from(ENVELOPE_COUNTER_MAX)
                + 1) as u16;
        }
        self.phase = (remainder % u64::from(ENVELOPE_DIVIDER)) as u8;
    }
}

pub(super) fn increment(rate: u8, counter: u16) -> u16 {
    if rate < 2 {
        return 0;
    }
    let shift = 11_u8.saturating_sub(rate >> 2);
    if counter & ((1 << shift) - 1) != 0 {
        return 0;
    }
    let index = (counter >> shift) & 7;
    if rate < 48 {
        let pattern = match rate {
            2..=5 => 0,
            6..=7 => 2,
            _ => usize::from(rate & 3),
        };
        return u16::from(ENVELOPE_PATTERNS[pattern] >> index & 1);
    }
    if rate >= 60 {
        return 8;
    }
    let doubled = match rate & 3 {
        0 => false,
        1 => counter & 3 == 0,
        2 => counter & 1 == 0,
        _ => counter & 3 != 3,
    };
    (1 + u16::from(doubled)) << ((rate - 48) >> 2)
}

#[cfg(test)]
mod tests;

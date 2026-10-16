use super::constants::{LFO_AM_SHIFTS, LFO_DIVIDERS, LFO_PM_WEIGHTS};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Lfo {
    pub(super) control: u8,
    pub(super) counter: u8,
    pub(super) quotient: u8,
    pub(super) sampled: u8,
}

impl Lfo {
    pub(super) fn counter_histogram(&self, mut frames: u64) -> [u64; 128] {
        let mut counts = [0; 128];
        if frames == 0 {
            return counts;
        }
        if self.control & 8 == 0 {
            counts[usize::from(self.counter)] = 1;
            counts[0] += frames - 1;
            return counts;
        }
        let mut cursor = *self;
        while frames != 0 && cursor.quotient != 1 {
            counts[usize::from(cursor.counter)] += 1;
            cursor.clock();
            frames -= 1;
        }
        let divider = u64::from(LFO_DIVIDERS[usize::from(cursor.control & 7)]);
        let full = frames / (128 * divider);
        for count in &mut counts {
            *count += full * divider;
        }
        let remaining = frames % (128 * divider);
        let blocks = remaining / divider;
        for block in 0..blocks {
            counts[(u64::from(cursor.counter) + block) as usize & 127] += divider;
        }
        counts[(u64::from(cursor.counter) + blocks) as usize & 127] += remaining % divider;
        counts
    }

    pub(super) fn write(&mut self, value: u8) {
        self.control = value & 15;
    }

    pub(super) fn clock(&mut self) {
        self.clock_slot(0);
        self.clock_slot(23);
    }

    pub(super) fn clock_slot(&mut self, cycle: usize) {
        if cycle == 0 {
            self.sampled = self.counter;
        }
        let divider = LFO_DIVIDERS[usize::from(self.control & 7)];
        if self.quotient & divider == divider {
            self.quotient = 0;
            self.counter = (self.counter + 1) & 127;
        } else if cycle == 23 {
            self.quotient += 1;
        }
        if self.control & 8 == 0 {
            self.counter = 0;
        }
    }

    pub(super) fn advance(&mut self, mut clocks: u64) {
        let divider = LFO_DIVIDERS[usize::from(self.control & 7)];
        while clocks != 0 && self.quotient != 1 {
            self.clock();
            clocks -= 1;
        }
        if clocks == 0 {
            return;
        }
        let elapsed = clocks + u64::from(self.quotient - 1);
        let steps = elapsed / u64::from(divider);
        let previous_steps = (elapsed - 1) / u64::from(divider);
        self.sampled = if clocks == 1 {
            self.counter
        } else if self.control & 8 != 0 {
            ((u64::from(self.counter) + previous_steps) & 127) as u8
        } else {
            0
        };
        self.quotient = (elapsed % u64::from(divider)) as u8 + 1;
        self.counter = if self.control & 8 != 0 {
            ((u64::from(self.counter) + steps) & 127) as u8
        } else {
            0
        };
    }
}

pub(super) fn amplitude(counter: u8, sensitivity: u8) -> u16 {
    let triangle = if counter < 64 {
        2 * (63 - counter)
    } else {
        2 * (counter - 64)
    };
    u16::from(triangle >> LFO_AM_SHIFTS[usize::from(sensitivity)])
}

pub(super) fn frequency(fnum: u16, counter: u8, sensitivity: u8) -> u16 {
    let step = counter >> 2;
    let quarter = (step & 7) ^ if step & 8 == 0 { 0 } else { 7 };
    let depth = sensitivity.min(5);
    let weight = LFO_PM_WEIGHTS[usize::from(depth)][usize::from(quarter)];
    let high = fnum >> 4;
    let contribution = match weight {
        0 => 0,
        1 => high >> 2,
        2 => high >> 1,
        3 => (high >> 1) + (high >> 2),
        4 => high,
        5 => high + (high >> 2),
        _ => high + (high >> 1),
    };
    let magnitude = (contribution << sensitivity.saturating_sub(5)) >> 2;
    let offset = if counter & 64 == 0 {
        magnitude as i32
    } else {
        -(magnitude as i32)
    };
    ((i32::from(fnum) * 2 + offset) & 0xfff) as u16
}

#[cfg(test)]
mod tests;

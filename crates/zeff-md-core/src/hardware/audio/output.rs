use super::constants::OUTPUT_CHANNELS;
mod kernel;
use kernel::KERNEL;

const TAPS: usize = 33;
const PHASES: usize = 32;
const BITS: u32 = 22;
const UNIT: i64 = 1 << BITS;
const INTERPOLATION_BITS: u32 = 16;
pub(super) const HISTORY: usize = TAPS + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Output {
    pending: [[i64; OUTPUT_CHANNELS]; HISTORY],
    integrator: [i64; OUTPUT_CHANNELS],
    level: [i32; OUTPUT_CHANNELS],
    cursor: usize,
}

impl Output {
    pub(super) fn at_level(level: [i32; OUTPUT_CHANNELS]) -> Self {
        Self {
            pending: [[0; OUTPUT_CHANNELS]; HISTORY],
            integrator: level.map(|value| i64::from(value) * UNIT),
            level,
            cursor: 0,
        }
    }

    pub(super) fn change_level(&mut self, level: [i32; OUTPUT_CHANNELS], phase: u64, master: u64) {
        let delta = std::array::from_fn::<_, OUTPUT_CHANNELS, _>(|side| {
            i64::from(level[side] - self.level[side])
        });
        self.level = level;
        if delta == [0; OUTPUT_CHANNELS] {
            return;
        }
        let offset = (phase / master) as usize;
        let position = (phase % master) * PHASES as u64;
        let index = (position / master) as usize;
        let fraction = (((position % master) << INTERPOLATION_BITS) / master) as i64;
        let mut sum = 0;
        for (tap, (&first, &second)) in KERNEL[index].iter().zip(&KERNEL[index + 1]).enumerate() {
            let first = i64::from(first);
            let second = i64::from(second);
            let weight = first + (((second - first) * fraction) >> INTERPOLATION_BITS);
            sum += weight;
            let slot = (self.cursor + offset + tap) % HISTORY;
            for (side, delta) in delta.into_iter().enumerate() {
                self.pending[slot][side] += delta * weight;
            }
        }
        // Fractional interpolation must retain an exact unit step to prevent DC drift.
        let center = (self.cursor + offset + TAPS / 2) % HISTORY;
        for (side, delta) in delta.into_iter().enumerate() {
            self.pending[center][side] += delta * (UNIT - sum);
        }
    }

    pub(super) fn sample(&mut self) -> [i16; OUTPUT_CHANNELS] {
        for (integrator, pending) in self
            .integrator
            .iter_mut()
            .zip(&mut self.pending[self.cursor])
        {
            *integrator += *pending;
            *pending = 0;
        }
        self.cursor = (self.cursor + 1) % HISTORY;
        self.integrator.map(|value| {
            ((value + UNIT / 2) >> BITS).clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
        })
    }
}

#[cfg(test)]
mod tests;

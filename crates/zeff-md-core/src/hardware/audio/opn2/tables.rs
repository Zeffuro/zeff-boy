use super::constants::{ENVELOPE_MAX, PHASE_BITS, PHASE_MASK, SINE_BITS, SINE_QUARTER};

const FIXED_BITS: u32 = 48;
const FIXED_ONE: i128 = 1 << FIXED_BITS;
const HALF_PI: i128 = 442_139_859_501_778;
const POWER_STEP: i128 = 280_713_884_160_287;
const LOG_ROUNDING_BOUNDARY: i128 = 281_094_172_843_150;
const LOG_BITS: u32 = 8;
const LOG_MASK: u32 = (1 << LOG_BITS) - 1;
const LOG_SINE: [u16; SINE_QUARTER] = log_sine_table();
const POWER: [u16; SINE_QUARTER] = power_table();

const fn log_sine_table() -> [u16; SINE_QUARTER] {
    let mut table = [0; SINE_QUARTER];
    let mut index = 0;
    while index < table.len() {
        let angle = HALF_PI * (2 * index + 1) as i128 / (2 * SINE_QUARTER) as i128;
        let square = (angle * angle) >> FIXED_BITS;
        let mut term = angle;
        let mut sine = angle;
        let mut order = 1;
        while order <= 8 {
            term = -((term * square) >> FIXED_BITS) / ((2 * order) * (2 * order + 1));
            sine += term;
            order += 1;
        }
        let mut boundary = LOG_ROUNDING_BOUNDARY;
        while sine < boundary {
            table[index] += 1;
            boundary = (boundary * POWER_STEP) >> FIXED_BITS;
        }
        index += 1;
    }
    table
}

const fn power_table() -> [u16; SINE_QUARTER] {
    let mut table = [0; SINE_QUARTER];
    let mut level = FIXED_ONE;
    let mut index = 0;
    while index < table.len() {
        level = (level * POWER_STEP) >> FIXED_BITS;
        table[index] = ((level * 2048 + FIXED_ONE / 2) >> FIXED_BITS) as u16;
        index += 1;
    }
    table
}

pub(super) fn output(phase: u32, attenuation: u16) -> i32 {
    if attenuation >= ENVELOPE_MAX {
        return 0;
    }
    let phase = ((phase & PHASE_MASK) >> (PHASE_BITS - SINE_BITS)) as usize;
    let mut quarter = phase & (SINE_QUARTER - 1);
    if phase & SINE_QUARTER != 0 {
        quarter ^= SINE_QUARTER - 1;
    }
    let level = u32::from(LOG_SINE[quarter]) + (u32::from(attenuation) << 2);
    let magnitude = (i32::from(POWER[(level & LOG_MASK) as usize]) << 2) >> (level >> LOG_BITS);
    if phase & (2 * SINE_QUARTER) != 0 {
        -magnitude
    } else {
        magnitude
    }
}

#[cfg(test)]
mod tests;

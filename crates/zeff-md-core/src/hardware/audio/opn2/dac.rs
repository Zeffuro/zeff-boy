const CARRIER_SHIFT: u32 = 5;
pub(super) const OUTPUT_SCALE: i32 = 1 << CARRIER_SHIFT;
pub(super) const OUTPUT_MIN: i32 = -256;
pub(super) const OUTPUT_MAX: i32 = 255;
const CARRIERS_S1_S3_S2_S4: [u8; 8] = [8, 8, 8, 8, 12, 14, 14, 15];
const NEGATIVE_ENABLED_OFFSET: i32 = 7;
const NEGATIVE_DISABLED_OFFSET: i32 = 8;

pub(super) fn quantize(value: i32) -> i32 {
    (value >> CARRIER_SHIFT).clamp(OUTPUT_MIN, OUTPUT_MAX)
}

pub(super) fn carrier_sum(sum: i32, value: i32, physical: usize, algorithm: u8) -> i32 {
    if carrier(physical, algorithm) {
        (sum + quantize(value)).clamp(OUTPUT_MIN, OUTPUT_MAX)
    } else {
        sum
    }
}

pub(super) fn carrier(physical: usize, algorithm: u8) -> bool {
    CARRIERS_S1_S3_S2_S4[usize::from(algorithm)] & (1 << physical) != 0
}

#[cfg(test)]
pub(super) fn sum_carriers(physical_order: [i32; 4], algorithm: u8) -> i32 {
    let mask = CARRIERS_S1_S3_S2_S4[usize::from(algorithm)];
    let mut sum = 0;
    for (slot, value) in physical_order.into_iter().enumerate() {
        if mask & (1 << slot) != 0 {
            sum = (sum + quantize(value)).clamp(OUTPUT_MIN, OUTPUT_MAX);
        }
    }
    sum * OUTPUT_SCALE
}

pub(super) fn transfer(value: i32, enabled: bool) -> i32 {
    // Integrate the four DAC clocks and subtract the positive idle baseline.
    let output = match (enabled, value < 0) {
        (true, true) => value - NEGATIVE_ENABLED_OFFSET,
        (true, false) => value,
        (false, true) => -NEGATIVE_DISABLED_OFFSET,
        (false, false) => 0,
    };
    output * OUTPUT_SCALE
}

#[cfg(test)]
mod tests;

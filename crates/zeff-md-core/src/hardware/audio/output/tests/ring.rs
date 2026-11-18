use super::*;

fn scalar(output: &mut Output, level: [i32; 2], phase: u64, master: u64) {
    let delta = std::array::from_fn::<_, 2, _>(|side| i64::from(level[side] - output.level[side]));
    output.level = level;
    if delta == [0; 2] {
        return;
    }
    let offset = (phase / master) as usize;
    let position = (phase % master) * PHASES as u64;
    let index = (position / master) as usize;
    let fraction = (((position % master) << INTERPOLATION_BITS) / master) as i64;
    let mut sum = 0;
    for (tap, (&first, &second)) in KERNEL[index].iter().zip(&KERNEL[index + 1]).enumerate() {
        let weight = i64::from(first)
            + (((i64::from(second) - i64::from(first)) * fraction) >> INTERPOLATION_BITS);
        sum += weight;
        let slot = (output.cursor + offset + tap) % HISTORY;
        for (side, &change) in delta.iter().enumerate() {
            output.pending[slot][side] += change * weight;
        }
    }
    let center = (output.cursor + offset + TAPS / 2) % HISTORY;
    for (side, &change) in delta.iter().enumerate() {
        output.pending[center][side] += change * (UNIT - sum);
    }
}

fn compare(cursor: usize, phase: u64, master: u64, level: [i32; 2], signature: &mut u64) {
    let mut actual = Output::at_level([-1234, 4321]);
    actual.cursor = cursor;
    actual.pending = std::array::from_fn(|i| [i as i64 * 7919 - 531, 937 - i as i64 * 73]);
    let mut expected = actual;
    scalar(&mut expected, level, phase, master);
    actual.change_level(level, phase, master);
    assert_eq!(actual, expected);
    *signature =
        signature.wrapping_mul(0x100000001b3) ^ actual.pending[(cursor + 17) % HISTORY][0] as u64;
    for _ in 0..HISTORY + 1 {
        assert_eq!(actual.sample(), expected.sample());
    }
    assert_eq!(actual, expected);
}

pub(super) fn reference_signature() -> u64 {
    let mut signature = 0xcbf29ce484222325;
    for cursor in 0..HISTORY {
        for master in [53_693_175_u64, 53_203_424, 32 << 16] {
            for index in 0..=PHASES {
                let phase = index as u64 * master / PHASES as u64;
                for phase in [phase.saturating_sub(1), phase, phase + 1, phase + master] {
                    for level in [[-32768, 32767], [32767, -32768], [-1234, 4321], [0, 4321]] {
                        compare(cursor, phase, master, level, &mut signature);
                    }
                }
            }
        }
    }
    for index in 0..PHASES {
        for fraction in 0..=u16::MAX {
            let phase = ((index as u64) << 16) | u64::from(fraction);
            compare(
                index % HISTORY,
                phase,
                32 << 16,
                [-31000, 30000],
                &mut signature,
            );
        }
    }
    for master in [53_693_175_u64, 53_203_424] {
        for rate in [8000_u64, 11025, 22050, 32000, 44100, 48000, 96000, 192000] {
            let mut actual = Output::at_level([0; 2]);
            let mut expected = actual;
            for event in 0..4096 {
                let phase = (event * 7919) % (master + rate);
                let level = [event as i32 % 65536 - 32768, 32767 - event as i32 % 65536];
                actual.change_level(level, phase, master);
                scalar(&mut expected, level, phase, master);
                assert_eq!(actual.sample(), expected.sample());
                assert_eq!(actual, expected);
                if event % 257 == 0 {
                    actual = Output::at_level(level);
                    expected = actual;
                }
            }
        }
    }
    signature
}

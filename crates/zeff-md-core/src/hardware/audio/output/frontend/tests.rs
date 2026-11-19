use super::*;

fn roundtrip(output: &Output) -> Result<Output> {
    let mut writer = StateWriter::new();
    output.write_frontend_state(&mut writer);
    Output::read_frontend_state(&mut StateReader::new(&writer.into_bytes()))
}

#[test]
fn kernel_keeps_event_count_independent_history_bound() {
    for row in KERNEL {
        assert_eq!(row.into_iter().map(i64::from).sum::<i64>(), UNIT);
    }
    let magnitude = KERNEL
        .iter()
        .flatten()
        .map(|value| i128::from(*value).abs())
        .max()
        .unwrap();
    let variation = (0..TAPS)
        .map(|tap| {
            KERNEL
                .windows(2)
                .map(|rows| (i128::from(rows[1][tap]) - i128::from(rows[0][tap])).abs())
                .sum::<i128>()
        })
        .max()
        .unwrap();
    let states = PHASES as i128 * (1_i128 << INTERPOLATION_BITS);
    let interval = 32768
        * (2 * (magnitude + TAPS as i128 - 1) + variation + (TAPS as i128 - 1) * (states - 1));
    let reachable = (HISTORY as i128 + 1) * interval;
    assert_eq!(reachable, 88_075_090_001_920);
    assert!(reachable < (1_i128 << 47));
    let future = MAX_PENDING + reachable;
    assert!(future + 65535 * 32 < (1_i128 << 49));
    assert!(
        32768 * i128::from(UNIT) + HISTORY as i128 * future + i128::from(UNIT / 2) < (1_i128 << 54)
    );
}

#[test]
fn cancellation_cannot_bypass_pending_magnitude_or_conservation() {
    for value in [i64::MIN, i64::MAX, (MAX_PENDING + 1) as i64] {
        let mut output = Output::at_level([0; 2]);
        output.pending[0][0] = value;
        output.pending[1][0] = value.saturating_neg();
        assert!(roundtrip(&output).is_err());
    }
    let mut output = Output::at_level([0; 2]);
    output.integrator[1] = 1;
    assert!(roundtrip(&output).is_err());
    output.integrator[1] = 0;
    output.level[0] = 32768;
    assert!(roundtrip(&output).is_err());
    output.level[0] = 0;
    output.cursor = HISTORY;
    assert!(roundtrip(&output).is_err());
}

struct Wide {
    pending: [[i128; 2]; HISTORY],
    integrator: [i128; 2],
    level: [i32; 2],
    cursor: usize,
}

impl Wide {
    fn change(&mut self, level: [i32; 2], phase: u64, master: u64) {
        let delta = std::array::from_fn::<_, 2, _>(|side| {
            i128::from(level[side]) - i128::from(self.level[side])
        });
        self.level = level;
        let position = phase % master * PHASES as u64;
        let index = (position / master) as usize;
        let fraction = ((position % master) << INTERPOLATION_BITS) / master;
        let mut weights = [0_i128; TAPS];
        for (tap, weight) in weights.iter_mut().enumerate() {
            let first = i128::from(KERNEL[index][tap]);
            *weight = first
                + (((i128::from(KERNEL[index + 1][tap]) - first) * i128::from(fraction))
                    >> INTERPOLATION_BITS);
        }
        let correction = i128::from(UNIT) - weights.iter().sum::<i128>();
        weights[TAPS / 2] += correction;
        for (tap, weight) in weights.into_iter().enumerate() {
            let slot = (self.cursor + (phase / master) as usize + tap) % HISTORY;
            for (side, change) in delta.into_iter().enumerate() {
                self.pending[slot][side] += change * weight;
            }
        }
    }

    fn sample(&mut self) -> [i16; 2] {
        for side in 0..2 {
            self.integrator[side] += self.pending[self.cursor][side];
            self.pending[self.cursor][side] = 0;
        }
        self.cursor = (self.cursor + 1) % HISTORY;
        self.integrator.map(|value| {
            ((value + i128::from(UNIT / 2)) >> BITS)
                .clamp(i128::from(i16::MIN), i128::from(i16::MAX)) as i16
        })
    }
}

#[test]
fn admitted_boundary_history_and_future_steps_match_i128_reference() {
    let mut initial = Output::at_level([0; 2]);
    for (index, frame) in initial.pending.iter_mut().enumerate() {
        *frame = [MAX_PENDING as i64 * if index % 2 == 0 { 1 } else { -1 }; 2];
    }
    let mut output = roundtrip(&initial).unwrap();
    let mut reference = Wide {
        pending: initial.pending.map(|frame| frame.map(i128::from)),
        integrator: initial.integrator.map(i128::from),
        level: initial.level,
        cursor: initial.cursor,
    };
    for step in 0..HISTORY * 3 {
        let phase = [0, 1, 996, 997][step % 4];
        for index in 0..101 {
            let level = if index % 2 == 0 {
                [i32::from(i16::MIN), i32::from(i16::MAX)]
            } else {
                [i32::from(i16::MAX), i32::from(i16::MIN)]
            };
            output.change_level(level, phase, 997);
            reference.change(level, phase, 997);
        }
        assert_eq!(output.sample(), reference.sample());
        assert_eq!(
            output.pending.map(|frame| frame.map(i128::from)),
            reference.pending
        );
        assert_eq!(output.integrator.map(i128::from), reference.integrator);
    }
}

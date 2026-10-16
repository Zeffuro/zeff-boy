use super::*;

#[test]
fn every_fractional_step_settles_exactly_without_dc_drift() {
    for phase in 0..2048 {
        let mut output = Output::at_level([0; 2]);
        output.change_level([8128, -8416], phase, 2048);
        let samples: Vec<_> = (0..HISTORY).map(|_| output.sample()).collect();
        assert_eq!(samples[HISTORY - 1], [8128, -8416]);
        assert_eq!(output.integrator, [8128 * UNIT, -8416 * UNIT]);
        assert!(output.pending.iter().all(|value| *value == [0; 2]));
        assert!(samples.iter().any(|value| *value != [8128, -8416]));
    }
}

#[test]
fn an_event_at_the_emission_boundary_matches_the_next_grid_origin() {
    let mut before = Output::at_level([0; 2]);
    before.change_level([1234, -5678], 1000, 1000);
    assert_eq!(before.sample(), [0; 2]);
    let mut after = Output::at_level([0; 2]);
    after.sample();
    after.change_level([1234, -5678], 0, 1000);
    for _ in 0..HISTORY * 2 {
        assert_eq!(before.sample(), after.sample());
    }
    assert_eq!(before, after);
}

#[test]
fn repeated_fractional_deltas_do_not_accumulate_rounding_error() {
    let mut output = Output::at_level([0; 2]);
    for index in 0..20_000 {
        output.change_level(
            [index % 13001 - 6500, 5000 - index % 12001],
            index as u64 % 997,
            997,
        );
        output.sample();
    }
    output.change_level([0; 2], 331, 997);
    for _ in 0..HISTORY {
        output.sample();
    }
    assert_eq!(output.integrator, [0; 2]);
    assert!(output.pending.iter().all(|value| *value == [0; 2]));
}

#[test]
fn checkpoint_preserves_pending_stereo_tails() {
    let mut original = Output::at_level([23, -17]);
    original.change_level([-31_000, 30_000], 351, 1000);
    for _ in 0..7 {
        original.sample();
    }
    let mut restored = original;
    for _ in 0..HISTORY * 2 {
        assert_eq!(restored.sample(), original.sample());
    }
    assert_eq!(restored, original);
}

#[test]
fn reconstruction_suppresses_a_native_rate_sampling_image() {
    let native = 53_267_u64;
    for rate in [44_100_u64, 48_000] {
        let mut output = Output::at_level([0; 2]);
        let frequency = 17_686.0;
        let count = rate as usize;
        let mut samples = Vec::with_capacity(count);
        let mut event = 0_u64;
        for index in 0..count {
            while event * rate <= (index as u64 + 1) * native {
                let phase = event * rate - index as u64 * native;
                let level = (8000.0
                    * (std::f64::consts::TAU * frequency * event as f64 / native as f64).sin())
                .round() as i32;
                output.change_level([level; 2], phase, native);
                event += 1;
            }
            samples.push(f64::from(output.sample()[0]));
        }
        let image = rate as f64 - (native as f64 - frequency);
        let power = |frequency: f64| {
            let mut real = 0.0;
            let mut imaginary = 0.0;
            for (index, sample) in samples.iter().enumerate().skip(100) {
                let phase = std::f64::consts::TAU * frequency * index as f64 / rate as f64;
                real += sample * phase.cos();
                imaginary += sample * phase.sin();
            }
            real * real + imaginary * imaginary
        };
        assert!(power(image) < power(frequency) * 0.00001);
    }
}

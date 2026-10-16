use super::*;

#[test]
fn integer_tables_match_independent_logarithmic_equations() {
    for index in 0..SINE_QUARTER {
        let angle = (index as f64 + 0.5) * std::f64::consts::FRAC_PI_2 / SINE_QUARTER as f64;
        let logarithm = (-256.0 * angle.sin().log2()).round() as u16;
        let power = (2048.0 * 2.0_f64.powf(-(index as f64 + 1.0) / 256.0)).round() as u16;
        assert_eq!(LOG_SINE[index], logarithm, "sine entry {index}");
        assert_eq!(POWER[index], power, "power entry {index}");
    }
}

#[test]
fn every_phase_obeys_quantized_silence_and_ignores_subphase_bits() {
    for index in 0..1024 {
        let phase = index << (PHASE_BITS - SINE_BITS);
        let first_silent = (0..=ENVELOPE_MAX)
            .find(|&level| output(phase, level) == 0)
            .unwrap();
        assert!(first_silent <= 832);
        for level in first_silent..=ENVELOPE_MAX {
            assert_eq!(output(phase, level), 0);
        }
        assert_eq!(output(phase, u16::MAX), 0);
        for level in [0, 64, 128, 831] {
            assert_eq!(output(phase, level), output(phase | 1023, level));
            assert_eq!(
                output(phase, level),
                output(phase.wrapping_add(1 << PHASE_BITS), level)
            );
        }
    }
}

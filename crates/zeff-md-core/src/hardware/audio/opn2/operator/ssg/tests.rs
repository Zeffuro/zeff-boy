use super::super::{Envelope, Operator};
use super::*;

#[test]
fn eight_shapes_distinguish_repeat_alternate_and_held_output() {
    let expected = [
        (true, false, false, true),
        (false, false, false, false),
        (true, true, false, false),
        (false, true, true, false),
        (true, true, false, true),
        (false, true, true, false),
        (true, false, false, false),
        (false, false, false, false),
    ];
    for (shape, (repeat, inverted, hold_up, reset)) in (8..16).zip(expected) {
        let mut ssg = Ssg::default();
        ssg.clock(shape, 511, true, true);
        assert!(!ssg.repeat && !ssg.reset);
        ssg.clock(shape, 512, true, true);
        assert_eq!(
            (ssg.repeat, ssg.inverted, ssg.hold_up, ssg.reset),
            (repeat, inverted, hold_up, reset)
        );
        assert_eq!(ssg.level(512), if inverted { 0 } else { 512 });
    }
}

#[test]
fn alternate_reverses_each_crossing_and_disabled_shapes_clear_at_the_latch() {
    let mut ssg = Ssg::default();
    ssg.clock(10, 512, true, true);
    assert!(ssg.inverted);
    ssg.clock(10, 0, true, true);
    assert!(ssg.inverted);
    ssg.clock(10, 512, true, true);
    assert!(!ssg.inverted);
    for mode in 0..8 {
        ssg.clock(15, 512, true, true);
        ssg.clock(mode, 512, true, true);
        assert_eq!(ssg, Ssg::default());
    }
}

#[test]
fn inverted_keyoff_converts_internal_level_before_the_next_latch() {
    let mut op = Operator {
        attenuation: 128,
        envelope: Envelope::Sustain,
        keyed: true,
        ..Operator::default()
    };
    op.pipeline.sampled = true;
    op.write(9, 12);
    op.select_rate((0, 0), 0, true);
    op.pipeline.sampled = false;
    op.clock_pipeline_envelope();
    assert_eq!(op.attenuation, 384);
    assert_eq!(op.envelope, Envelope::Release);
    assert!(op.ssg.inverted);
    op.generate_level(false);
    assert_eq!(op.pipeline.level, 128);
    op.select_rate((0, 0), 0, true);
    op.generate_level(false);
    assert_eq!(op.pipeline.level, 384);
}

#[test]
fn keyoff_repeat_wins_the_attack_state_but_not_a_new_phase_reset() {
    let mut op = Operator {
        attenuation: 512,
        envelope: Envelope::Sustain,
        keyed: true,
        ..Operator::default()
    };
    op.write(9, 10);
    op.select_rate((0, 0), 0, true);
    op.clock_pipeline_envelope();
    assert_eq!(op.envelope, Envelope::Attack);
    assert!(!op.keyed);
    assert!(!op.pipeline.reset);
    assert_eq!(op.attenuation, 0);
}

#[test]
fn repeating_saw_holds_phase_reset_even_after_keyoff_at_full_attenuation() {
    let mut op = Operator::default();
    op.write(9, 8);
    op.select_rate((0, 0), 0, true);
    op.clock_pipeline_envelope();
    assert!(op.pipeline.reset);
    assert!(!op.quiet_pipeline());
    let mut inverted = Ssg::default();
    inverted.clock(12, 1023, true, true);
    assert_eq!(inverted.level(1023), 513);
}

#[test]
fn linear_decay_and_release_are_four_times_normal_until_bit_nine() {
    for envelope in [Envelope::Decay, Envelope::Sustain, Envelope::Release] {
        let mut normal = Operator {
            attenuation: 480,
            envelope,
            ..Operator::default()
        };
        normal.pipeline.prepared.increment = 8;
        let mut accelerated = normal;
        accelerated.ssg.clock(9, 480, false, false);
        normal.clock_pipeline_envelope();
        accelerated.clock_pipeline_envelope();
        assert_eq!(normal.attenuation, 488);
        assert_eq!(accelerated.attenuation, 512);
        accelerated.clock_pipeline_envelope();
        assert_eq!(accelerated.attenuation, 1023);
    }
}

#[test]
fn packed_state_preserves_live_write_latches_and_rejects_unknown_bits() {
    for flags in 0..64 {
        let bytes = [flags];
        let state = Ssg::read_state(&mut StateReader::new(&bytes)).unwrap();
        let mut writer = StateWriter::new();
        state.write_state(&mut writer);
        assert_eq!(writer.into_bytes(), bytes);
    }
    for flags in [64, 128, 255] {
        assert!(Ssg::read_state(&mut StateReader::new(&[flags])).is_err());
    }
    assert!(Ssg::read_state(&mut StateReader::new(&[])).is_err());
}

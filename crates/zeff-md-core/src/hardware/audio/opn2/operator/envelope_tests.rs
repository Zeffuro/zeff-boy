use super::*;
use crate::hardware::audio::opn2::envelope_clock::EnvelopeClock;

fn advance(operator: &mut Operator, clock: &mut EnvelopeClock, frequency: (u16, u8)) {
    if let Some(counter) = clock.clock() {
        operator.clock_envelope(frequency, counter);
    }
    operator.clock(frequency, 0);
}

#[test]
fn signed_attack_rounding_and_fast_step_curve_match_digital_vectors() {
    for (attenuation, expected) in [(16, 14), (30, 28), (31, 29), (32, 29)] {
        let mut operator = Operator {
            attenuation,
            envelope: Envelope::Attack,
            ..Operator::default()
        };
        operator.write(5, 24);
        operator.clock_envelope((0, 0), 1);
        assert_eq!(operator.attenuation, expected);
    }
    let mut operator = Operator::default();
    operator.write(5, 30);
    operator.key(true, (0, 0));
    for (counter, expected) in (1..).zip([511, 255, 127, 63, 31, 15, 7, 3, 1, 0]) {
        operator.clock_envelope((0, 0), counter);
        assert_eq!(operator.attenuation, expected);
    }
    assert_eq!(operator.envelope, Envelope::Decay);
}

#[test]
fn key_edges_and_live_rate_writes_use_the_existing_shared_position() {
    let mut clock = EnvelopeClock::default();
    let mut early = Operator::default();
    early.write(5, 31);
    early.write(6, 22);
    early.write(8, 0xf0);
    early.key(true, (0, 0));
    advance(&mut early, &mut clock, (0, 0));
    let mut late = Operator {
        parameters: early.parameters,
        ..Operator::default()
    };
    late.key(true, (0, 0));
    clock.clock();
    let counter = clock.clock().unwrap();
    for operator in [&mut early, &mut late] {
        operator.clock_envelope((0, 0), counter);
    }
    assert_eq!(early.attenuation, 1);
    assert_eq!(late.attenuation, 1);
    early.key(false, (0, 0));
    early.key(true, (0, 0));
    assert_eq!(clock.counter, 1);
    early.write(6, 0xde);
    early.clock_envelope((0, 0), 2);
    assert_eq!(early.attenuation, 8);
    early.write(6, 1);
    early.clock_envelope((0, 0), 4095);
    assert_eq!(early.attenuation, 8);
    early.clock_envelope((0, 0), 4096);
    assert_eq!(early.attenuation, 8);
    early.clock_envelope((0, 0), 6144);
    assert_eq!(early.attenuation, 9);
}

#[test]
fn decay_keeps_overshoot_and_sustain_is_one_way() {
    let mut operator = Operator {
        attenuation: 28,
        envelope: Envelope::Decay,
        ..Operator::default()
    };
    operator.write(6, 30);
    operator.write(8, 0x10);
    operator.clock_envelope((0, 0), 1);
    assert_eq!(operator.attenuation, 36);
    assert_eq!(operator.envelope, Envelope::Decay);
    operator.clock_envelope((0, 0), 2);
    assert_eq!(operator.attenuation, 36);
    assert_eq!(operator.envelope, Envelope::Sustain);
    operator.write(8, 0xf0);
    operator.clock_envelope((0, 0), 3);
    assert_eq!(operator.attenuation, 36);
    assert_eq!(operator.envelope, Envelope::Sustain);
    assert_eq!(operator.sustain_target(), 992);
}

#[test]
fn sustain_matches_a_level_window_instead_of_an_ordering_threshold() {
    for (sustain, attenuation, expected, matched) in [
        (0, 15, 15, true),
        (0, 16, 24, false),
        (1, 31, 39, false),
        (1, 32, 32, true),
        (1, 47, 47, true),
        (1, 48, 56, false),
        (15, 991, 999, false),
        (15, 992, 992, true),
        (15, 1007, 1007, true),
    ] {
        let mut operator = Operator {
            attenuation,
            envelope: Envelope::Decay,
            keyed: true,
            ..Operator::default()
        };
        operator.write(6, 31);
        operator.write(8, sustain << 4);
        operator.clock_envelope((0, 0), 1);
        assert_eq!(operator.attenuation, expected);
        assert_eq!(
            operator.envelope,
            if matched {
                Envelope::Sustain
            } else {
                Envelope::Decay
            }
        );
    }
}

#[test]
fn attack_entry_uses_scaled_rate_and_preserves_zero_rate() {
    for (ar, fnum, block, immediate) in [
        (30, 0x37f, 3, false),
        (30, 0x400, 3, false),
        (30, 0x000, 4, true),
        (0x5e, 0x37f, 1, false),
        (0x5e, 0x000, 2, true),
        (0xd8, 0x3ff, 3, false),
        (0xd8, 0x400, 3, true),
        (0xdc, 644, 3, true),
        (0xdc, 1148, 2, true),
        (31, 0, 0, true),
        (0xc0, 0x7ff, 7, false),
    ] {
        let mut operator = Operator::default();
        operator.write(5, ar);
        operator.key(true, (fnum, block));
        assert_eq!(
            operator.attenuation == 0,
            immediate,
            "AR={ar:02x} fnum={fnum} block={block}"
        );
        assert_eq!(
            operator.envelope,
            if immediate {
                Envelope::Decay
            } else {
                Envelope::Attack
            }
        );
    }
}

#[test]
fn zero_level_rekey_enters_decay_independently_of_attack_rate() {
    for attenuation in [0, 1, 512, ENVELOPE_MAX] {
        for attack in [0, 1, 17, 31] {
            let mut operator = Operator {
                attenuation,
                phase: 123,
                envelope: Envelope::Release,
                ..Operator::default()
            };
            operator.write(5, attack);
            operator.write(6, 20);
            operator.write(8, 0xf0);
            operator.key(true, (0, 0));
            let decay = attenuation == 0 || attack == 31;
            assert_eq!(
                operator.envelope,
                if decay {
                    Envelope::Decay
                } else {
                    Envelope::Attack
                }
            );
            assert_eq!(operator.attenuation, if decay { 0 } else { attenuation });
            assert_eq!(operator.phase, 0);
            operator.phase = 456;
            let held = operator;
            operator.key(true, (0, 0));
            assert_eq!(operator, held);
            for counter in 1..=4095 {
                operator.clock_envelope((0, 0), counter);
            }
            if decay {
                assert!(operator.attenuation > 0);
            } else if attack == 0 {
                assert_eq!(operator.attenuation, attenuation);
                assert_eq!(operator.envelope, Envelope::Attack);
            }
        }
    }
}

#[test]
fn raising_attack_rate_to_62_stalls_until_rate_changes_or_key_off() {
    let mut operator = Operator::default();
    operator.write(5, 24);
    operator.key(true, (0, 0));
    let mut clock = EnvelopeClock::default();
    for _ in 0..60 {
        advance(&mut operator, &mut clock, (0, 0));
    }
    let attenuation = operator.attenuation;
    assert!(attenuation > 0 && attenuation < ENVELOPE_MAX);
    operator.write(5, 30);
    for _ in 0..60 {
        advance(&mut operator, &mut clock, (0, 4));
    }
    assert_eq!(operator.envelope, Envelope::Attack);
    assert_eq!(operator.attenuation, attenuation);
    operator.key(true, (0, 4));
    assert_eq!(operator.attenuation, attenuation);
    operator.write(5, 24);
    for _ in 0..60 {
        advance(&mut operator, &mut clock, (0, 0));
    }
    assert!(operator.attenuation < attenuation);
    operator.key(false, (0, 0));
    assert_eq!(operator.envelope, Envelope::Release);
    operator.write(5, 31);
    operator.key(true, (0, 0));
    assert_eq!(operator.attenuation, 0);
}

#[test]
fn special_channel_attack_entry_uses_each_operators_frequency() {
    use crate::hardware::audio::opn2::Opn2;
    let mut normal = Opn2::default();
    for (address, value) in [
        (0xb2, 7),
        (0x52, 30),
        (0x56, 30),
        (0x5a, 30),
        (0x5e, 30),
        (0xa6, 4),
        (0xa2, 0),
        (0xac, 0x24),
        (0xa8, 0),
        (0xad, 0x24),
        (0xa9, 0),
        (0xae, 4),
        (0xaa, 0),
    ] {
        normal.write_port(0, address);
        normal.write_port(1, value);
    }
    let mut special = normal;
    special.write_port(0, 0x27);
    special.write_port(1, 0x40);
    for (slot, mask, fast) in [
        (2, 0x12, true),
        (14, 0x22, false),
        (8, 0x42, true),
        (20, 0x82, false),
    ] {
        let mut slow = normal;
        let mut scaled = special;
        for chip in [&mut slow, &mut scaled] {
            chip.write_port(0, 0x28);
            chip.write_port(1, mask);
            chip.advance(144 * 2);
        }
        assert_eq!(scaled.slot(slot).control_snapshot().0 == 0, fast);
        assert!(slow.slot(slot).control_snapshot().0 > 0);
    }
}

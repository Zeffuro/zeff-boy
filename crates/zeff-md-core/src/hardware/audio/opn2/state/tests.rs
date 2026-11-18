use super::*;

fn bytes(chip: &Opn2) -> Vec<u8> {
    let mut w = Writer::new();
    chip.write_state(&mut w);
    w.into_bytes()
}

#[test]
fn state_preserves_pending_commands_and_rejects_cross_cursor_disagreement() {
    let mut chip = Opn2::default();
    chip.advance(137);
    chip.scheduler.key(0xf5);
    let saved = bytes(&chip);
    let mut restored = Opn2::read_state(&mut Reader::new(&saved)).unwrap();
    assert_eq!(restored, chip);
    for elapsed in [0, 1, 5, 6, 7, 143, 144, 145, 1024] {
        chip.advance(elapsed);
        restored.advance(elapsed);
        assert_eq!(restored, chip);
    }
    chip.scheduler.step = (chip.scheduler.step + 1) % scheduler::CLOCK_END;
    assert!(Opn2::read_state(&mut Reader::new(&bytes(&chip))).is_err());
}

#[test]
fn active_slot_and_fractional_boundaries_preserve_saved_future() {
    let mut chip = Opn2::default();
    for (address, value) in [
        (0x30, 1),
        (0x50, 31),
        (0x60, 20),
        (0x80, 0xf5),
        (0xb0, 7),
        (0xb4, 0xc0),
        (0xa4, 0x24),
        (0xa0, 0x69),
        (0x28, 0x10),
    ] {
        chip.write_port(0, address);
        chip.write_port(1, value);
    }
    chip.advance(144 * 6);
    assert!(chip.output.iter().any(|&value| value != 0));
    let mut sounded = false;
    let mut released = false;
    let mut rekeyed = false;
    for clock in 0..=720 {
        if clock == 97 || clock == 331 {
            chip.write_port(0, 0x28);
            chip.write_port(1, if clock == 97 { 0 } else { 0x10 });
        }
        let saved = bytes(&chip);
        let mut restored = Opn2::read_state(&mut Reader::new(&saved)).unwrap();
        let mut expected = chip;
        assert_eq!(restored, expected);
        for elapsed in [0, 1, 5, 6, 7, 143, 144] {
            restored.advance(elapsed);
            expected.advance(elapsed);
            assert_eq!(restored, expected, "clock {clock} elapsed {elapsed}");
        }
        chip.advance(1);
        sounded |= chip.output.iter().any(|&value| value != 0);
        let mut writer = Writer::new();
        chip.slot(0).write_state(&mut writer);
        let keyed = writer.into_bytes()[14] != 0;
        released |= clock > 97 && !keyed;
        rekeyed |= released && keyed && chip.slot(0).pipeline.reset;
    }
    assert!(sounded);
    assert!(released && rekeyed);
    assert!(chip.slot(0).pipeline.sample.valid);
}

#[test]
fn rejects_unsafe_latches_timer_payloads_and_output_history() {
    let mut cases = Vec::new();
    let chip = Opn2 {
        address: 0x200,
        ..Opn2::default()
    };
    cases.push(chip);
    for envelope_clock in [
        EnvelopeClock {
            counter: constants::ENVELOPE_COUNTER_MAX + 1,
            phase: 0,
        },
        EnvelopeClock {
            counter: 0,
            phase: constants::ENVELOPE_DIVIDER,
        },
    ] {
        cases.push(Opn2 {
            envelope_clock,
            ..Opn2::default()
        });
    }
    for lfo in [
        Lfo {
            control: 16,
            ..Lfo::default()
        },
        Lfo {
            counter: 128,
            ..Lfo::default()
        },
        Lfo {
            quotient: 128,
            ..Lfo::default()
        },
    ] {
        cases.push(Opn2 {
            lfo,
            ..Opn2::default()
        });
    }
    let mut chip = Opn2::default();
    chip.timer_a.reload = TIMER_A_STEPS;
    cases.push(chip);
    let mut chip = Opn2::default();
    chip.slot_mut(0).pipeline.sampled = true;
    chip.slot_mut(0).pipeline.csm = true;
    cases.push(chip);
    let chip = Opn2 {
        phase: FM_CLOCK_DIVIDER,
        ..Opn2::default()
    };
    cases.push(chip);
    let mut chip = Opn2::default();
    chip.output[0] = i32::MAX;
    cases.push(chip);
    let mut chip = Opn2::default();
    chip.output_pan[2] = 1;
    cases.push(chip);
    for chip in cases {
        assert!(Opn2::read_state(&mut Reader::new(&bytes(&chip))).is_err());
    }
}

use super::*;

fn bytes(chip: &Opn2) -> Vec<u8> {
    let mut w = Writer::new();
    chip.write_state(&mut w);
    w.into_bytes()
}

fn legacy_bytes(chip: &Opn2, version: u32) -> Vec<u8> {
    let mut w = Writer::new();
    chip.write_state_version(&mut w, version);
    w.into_bytes()
}

#[test]
fn legacy_clock_epochs_migrate_without_accepting_invalid_old_states() {
    for (counter, expected) in [(0, 0), (1, 1), (4095, 4095), (4096, 1), (16383, 3)] {
        for phase in 0..constants::ENVELOPE_DIVIDER {
            let chip = Opn2 {
                envelope_clock: EnvelopeClock { counter, phase },
                ..Opn2::default()
            };
            let saved = legacy_bytes(&chip, 4);
            let mut reader = Reader::new(&saved);
            let restored = Opn2::read_state_version(
                &mut reader,
                crate::save_state::STATE_VERSION_WITH_ENVELOPE_CLOCK,
            )
            .unwrap();
            assert!(reader.is_exhausted());
            assert_eq!(restored.envelope_clock.counter, expected);
            assert_eq!(restored.envelope_clock.phase, phase);
            assert_eq!(
                Opn2::read_state(&mut Reader::new(&bytes(&restored))).unwrap(),
                restored
            );
            if counter > constants::ENVELOPE_COUNTER_MAX {
                assert!(Opn2::read_state(&mut Reader::new(&saved)).is_err());
            }
        }
    }
    for (counter, phase) in [(16384, 0), (0, constants::ENVELOPE_DIVIDER)] {
        let chip = Opn2 {
            envelope_clock: EnvelopeClock { counter, phase },
            ..Opn2::default()
        };
        assert!(
            Opn2::read_state_version(
                &mut Reader::new(&legacy_bytes(&chip, 4)),
                crate::save_state::STATE_VERSION_WITH_ENVELOPE_CLOCK,
            )
            .is_err()
        );
    }
}

#[test]
fn legacy_slot_migration_preserves_canonical_state_without_inventing_edges() {
    for version in 1..=5 {
        for phase in [0, 1, 5, 6, 137, 143] {
            let mut chip = Opn2 {
                phase,
                output: [37, -41, 0, 1, 2, 3],
                ..Opn2::default()
            };
            chip.channels[0].write_operator(0x50, 31);
            chip.channels[0].key(1);
            if version >= 4 {
                chip.envelope_clock = EnvelopeClock {
                    counter: 4095,
                    phase: 2,
                };
            }
            let saved = legacy_bytes(&chip, version);
            let mut reader = Reader::new(&saved);
            let restored = Opn2::read_state_version(&mut reader, version).unwrap();
            assert!(reader.is_exhausted());
            assert_eq!(legacy_bytes(&restored, version), saved);
            let clock = restored.envelope_clock;
            assert_eq!(
                restored.scheduler.step,
                72 * u32::from(clock.counter) + 24 * u32::from(clock.phase) + (phase / 6) as u32
            );
            assert_eq!(
                restored.scheduler.command,
                scheduler::Command {
                    channel: 255,
                    mask: 0
                }
            );
            assert_eq!(restored.scheduler.pending, None);
            assert!(!restored.scheduler.frequency_valid);
            assert_eq!(restored.scheduler.output, chip.output);
            assert_eq!(restored.scheduler.ready, 0);
            for slot in 0..scheduler::SLOTS {
                let operator = restored.slot(slot);
                let mut writer = Writer::new();
                operator.write_state_version(&mut writer, 5);
                let keyed = writer.into_bytes()[14] != 0;
                assert_eq!(operator.pipeline, operator::Pipeline::migrated(keyed));
            }
            assert_eq!(
                Opn2::read_state(&mut Reader::new(&bytes(&restored))).unwrap(),
                restored
            );
        }
    }
}

#[test]
fn legacy_warm_up_keeps_each_output_until_its_valid_channel_latch() {
    for phase in [0, 5, 6, 59, 137, 143] {
        let old_output = [37, -41, 23, -17, 11, -5];
        let old = Opn2 {
            phase,
            output: old_output,
            ..Opn2::default()
        };
        let mut chip =
            Opn2::read_state_version(&mut Reader::new(&legacy_bytes(&old, 5)), 5).unwrap();
        let mut first_publication = None;
        let mut published = 0;
        for elapsed in 1..=144 * 4 {
            let ready = chip.scheduler.ready;
            let phase = chip.phase;
            let previous = chip.output;
            chip.advance(1);
            for channel in 0..6 {
                if chip.output[channel] != previous[channel] {
                    assert_eq!(phase % 24, 5);
                    assert_ne!(ready & (1 << channel), 0);
                    assert_eq!(previous[channel], old_output[channel]);
                    assert_eq!(chip.output[channel], 0);
                    published |= 1 << channel;
                    first_publication.get_or_insert(elapsed);
                }
            }
            if chip.scheduler.ready != 0 {
                let saved = bytes(&chip);
                assert_eq!(Opn2::read_state(&mut Reader::new(&saved)).unwrap(), chip);
            }
        }
        assert!(first_publication.is_some());
        assert!(first_publication.unwrap() > 144);
        assert_eq!(published, 63);
        assert!(chip.output.iter().all(|&value| value == 0));
    }
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
        chip.slot(0).write_state_version(&mut writer, 5);
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
    for (steps, divider, reload, remaining, running) in [
        (1024, 144, 1024, 1, true),
        (256, 2304, 255, 0, true),
        (256, 2304, 255, 256 * 2304 + 1, false),
    ] {
        let mut w = Writer::new();
        w.write_u16(reload);
        w.write_u64(remaining);
        w.write_bool(running);
        assert!(
            timer::LegacyTimer::read_state(&mut Reader::new(&w.into_bytes()), steps, divider)
                .is_err()
        );
    }
}

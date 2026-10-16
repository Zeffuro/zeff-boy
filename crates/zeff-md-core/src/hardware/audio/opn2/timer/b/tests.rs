use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn scalar(timer: &mut TimerB, cycle: usize, slots: u64, mode: u8, flags: &mut u8) {
    for offset in 0..slots {
        timer.clock((cycle + offset as usize) % SLOTS, mode, flags);
    }
}

#[test]
fn stopped_divider_phase_survives_start_and_controls_the_first_flag() {
    for wait in 0..16 {
        let mut timer = TimerB {
            reload: 255,
            ..TimerB::default()
        };
        let mut flags = 1;
        scalar(&mut timer, 0, wait * SLOTS as u64, 0, &mut flags);
        assert_eq!(timer.subcounter, wait as u8);
        assert_eq!(timer.counter, 0);
        let mut clocks = 0;
        while flags & 2 == 0 {
            timer.clock(clocks % SLOTS, 0x0a, &mut flags);
            clocks += 1;
            assert!(clocks <= 400);
        }
        assert_eq!(clocks * 6, 162 + 144 * ((30 - wait as usize) % 16));
        assert_eq!(flags, 3);
    }
}

#[test]
fn reload_overflow_clear_and_load_have_separate_owners() {
    let mut timer = TimerB {
        reload: 255,
        subcounter: 14,
        ..TimerB::default()
    };
    let mut flags = 1;
    scalar(&mut timer, 0, 3, 0x0a, &mut flags);
    assert!(timer.load_lock && timer.reload_pending);
    assert_eq!(timer.subcounter, 15);
    timer.reload = 254;
    timer.clock(3, 0x0a, &mut flags);
    assert_eq!(timer.counter, 254);
    scalar(&mut timer, 4, 22, 0x0a, &mut flags);
    assert_eq!(timer.counter, 255);
    assert_eq!(timer.subcounter, 0);
    scalar(&mut timer, 2, 16 * SLOTS as u64, 0x0a, &mut flags);
    assert!(timer.overflow);
    assert_eq!(flags, 1);
    timer.clear(true);
    timer.clock(2, 0x0a, &mut flags);
    assert!(timer.reload_pending && !timer.clear);
    assert_eq!(flags, 1);
    timer.clock(3, 0x0a, &mut flags);
    assert_eq!(timer.counter, 254);
    flags = 3;
    timer.clear(true);
    timer.clear(false);
    timer.clock(4, 0x0a, &mut flags);
    assert_eq!(flags, 3);
    timer.clear(true);
    assert_eq!(flags, 3);
    timer.clock(5, 0x0a, &mut flags);
    assert_eq!(flags, 1);
}

#[test]
fn stop_start_between_load_samples_does_not_reload_or_reset_the_divider() {
    let mut timer = TimerB {
        reload: 254,
        ..TimerB::default()
    };
    let mut flags = 0;
    scalar(&mut timer, 0, 4, 0x0a, &mut flags);
    scalar(&mut timer, 4, 10, 0, &mut flags);
    let subcounter = timer.subcounter;
    scalar(&mut timer, 14, 13, 0x0a, &mut flags);
    assert!(timer.load_lock);
    assert_eq!(timer.subcounter, (subcounter + 1) % 16);
    assert_eq!(timer.counter, 254);
    assert!(!timer.reload_pending);
}

#[test]
fn projection_preserves_every_timer_field_at_all_divider_and_slot_phases() {
    for reload in [0, 1, 254, 255] {
        for initial in [0, 2, 10] {
            for mode in [0, 2, 8, 10] {
                for subcounter in 0..16 {
                    for cut in 0..SLOTS {
                        let mut original = TimerB {
                            reload,
                            subcounter,
                            ..TimerB::default()
                        };
                        let mut flags = 1;
                        scalar(
                            &mut original,
                            0,
                            (SLOTS * 3 + cut) as u64,
                            initial,
                            &mut flags,
                        );
                        original.clear(cut % 3 == 0);
                        original.reload = 255 - reload;
                        for slots in [0, 1, 23, 24, 25, 385, 1009] {
                            let mut fast = original;
                            let mut slow = original;
                            let mut a = flags;
                            let mut b = flags;
                            fast.advance_slots(cut, slots, mode, &mut a);
                            scalar(&mut slow, cut, slots, mode, &mut b);
                            assert_eq!(
                                (fast, a),
                                (slow, b),
                                "{reload}/{initial}/{mode}/{subcounter}/{cut}/{slots}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn huge_projection_composes_and_stopped_time_still_moves_the_divider() {
    for reload in [0, 1, 254, 255] {
        for mode in [0, 2, 8, 10] {
            for subcounter in 0..16 {
                for cut in 0..SLOTS {
                    let mut whole = TimerB {
                        reload,
                        subcounter,
                        ..TimerB::default()
                    };
                    let mut split = whole;
                    let mut a = 1;
                    let mut b = 1;
                    whole.advance_slots(cut, u64::MAX, mode, &mut a);
                    split.advance_slots(cut, u64::MAX - 1007, mode, &mut b);
                    let next = (cut as u64 + (u64::MAX - 1007) % SLOTS as u64) % SLOTS as u64;
                    split.advance_slots(next as usize, 1007, mode, &mut b);
                    assert_eq!((whole, a), (split, b));
                    let next = (cut as u64 + u64::MAX % SLOTS as u64) % SLOTS as u64;
                    let mut slow = whole;
                    whole.advance_slots(next as usize, 24 * 33, 10, &mut a);
                    scalar(&mut slow, next as usize, 24 * 33, 10, &mut b);
                    assert_eq!((whole, a), (slow, b));
                }
            }
        }
    }
}

#[test]
fn projection_settles_every_minimally_accepted_transient() {
    for subcounter in 0..16 {
        for bits in 0..16 {
            let bytes = [1, if bits & 4 == 0 { 255 } else { 0 }, subcounter, bits];
            let timer = TimerB::read_state(
                &mut StateReader::new(&bytes),
                crate::save_state::STATE_VERSION,
            )
            .unwrap();
            for cut in 0..SLOTS {
                for mode in [0, 10] {
                    for slots in [0, 1, 25, 1009] {
                        let (mut fast, mut slow) = (timer, timer);
                        let (mut a, mut b) = (3, 3);
                        fast.advance_slots(cut, slots, mode, &mut a);
                        scalar(&mut slow, cut, slots, mode, &mut b);
                        assert_eq!(
                            (fast, a),
                            (slow, b),
                            "{subcounter}/{bits}/{cut}/{mode}/{slots}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn legacy_codec_rejects_invalid_and_truncated_records() {
    for version in 1..=12 {
        for (reload, remaining, running) in [(256, 1, 0), (0, 589825, 0), (0, 0, 1), (0, 1, 2)] {
            let mut writer = StateWriter::new();
            writer.write_u16(reload);
            writer.write_u64(remaining);
            writer.write_u8(running);
            assert!(
                TimerB::read_state(&mut StateReader::new(&writer.into_bytes()), version).is_err()
            );
        }
        for len in 0..11 {
            assert!(TimerB::read_state(&mut StateReader::new(&[0; 11][..len]), version).is_err());
        }
    }
}

#[test]
fn running_legacy_first_flag_stays_inside_the_documented_residual_bound() {
    for remaining in [1_u64, 143, 144, 145, 2303, 2304, 2305, 589824] {
        for reload in [0, 1, 254, 255] {
            let mut writer = StateWriter::new();
            writer.write_u16(reload);
            writer.write_u64(remaining);
            writer.write_bool(true);
            let bytes = writer.into_bytes();
            let timer = TimerB::read_state(&mut StateReader::new(&bytes), 12).unwrap();
            for cut in 0..SLOTS {
                for fraction in 0..6 {
                    for (clocks, expected) in
                        [(remaining.saturating_sub(138), 0), (remaining + 149, 2)]
                    {
                        let mut projected = timer;
                        let mut flags = 0;
                        projected.advance_slots(cut, (clocks + fraction) / 6, 10, &mut flags);
                        assert_eq!(
                            flags, expected,
                            "{remaining}/{reload}/{cut}/{fraction}/{clocks}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn old_countdowns_keep_scan_progress_without_claiming_stopped_phase_history() {
    for remaining in [0, 1, 143, 144, 145, 2303, 2304, 2305, 256 * 2304] {
        for running in [false, true] {
            if running && remaining == 0 {
                continue;
            }
            for version in 1..=12 {
                let mut writer = StateWriter::new();
                writer.write_u16(254);
                writer.write_u64(remaining);
                writer.write_bool(running);
                let bytes = writer.into_bytes();
                let timer = TimerB::read_state(&mut StateReader::new(&bytes), version).unwrap();
                let scans = remaining.div_ceil(144) as u16;
                if scans == 0 {
                    assert_eq!((timer.counter, timer.subcounter), (0, 0));
                } else {
                    assert_eq!(
                        (256 - u16::from(timer.counter)) * 16 - u16::from(timer.subcounter),
                        scans
                    );
                }
                assert_eq!(timer.load_lock, running);
                assert!(!timer.reload_pending && !timer.overflow && !timer.clear);
                let mut writer = StateWriter::new();
                timer.write_state(&mut writer, crate::save_state::STATE_VERSION);
                let saved = writer.into_bytes();
                assert_eq!(
                    TimerB::read_state(
                        &mut StateReader::new(&saved),
                        crate::save_state::STATE_VERSION
                    )
                    .unwrap(),
                    timer
                );
            }
        }
    }
}

#[test]
fn current_codec_retains_transients_and_rejects_invalid_phase_bits() {
    for subcounter in 0..16 {
        for bits in 0..16 {
            let counter = if bits & 4 != 0 { 0 } else { 255 };
            let bytes = [254, counter, subcounter, bits];
            let timer = TimerB::read_state(
                &mut StateReader::new(&bytes),
                crate::save_state::STATE_VERSION,
            )
            .unwrap();
            let mut writer = StateWriter::new();
            timer.write_state(&mut writer, crate::save_state::STATE_VERSION);
            assert_eq!(writer.into_bytes(), bytes);
        }
    }
    for bytes in [[0, 0, 16, 0], [0, 0, 0, 16], [0, 1, 0, 4]] {
        assert!(
            TimerB::read_state(
                &mut StateReader::new(&bytes),
                crate::save_state::STATE_VERSION
            )
            .is_err()
        );
    }
    for len in 0..4 {
        assert!(
            TimerB::read_state(
                &mut StateReader::new(&[0; 4][..len]),
                crate::save_state::STATE_VERSION
            )
            .is_err()
        );
    }
}

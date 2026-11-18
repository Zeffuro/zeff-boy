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
            let timer = TimerB::read_state(&mut StateReader::new(&bytes)).unwrap();
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
fn current_codec_retains_transients_and_rejects_invalid_phase_bits() {
    for subcounter in 0..16 {
        for bits in 0..16 {
            let counter = if bits & 4 != 0 { 0 } else { 255 };
            let bytes = [254, counter, subcounter, bits];
            let timer = TimerB::read_state(&mut StateReader::new(&bytes)).unwrap();
            let mut writer = StateWriter::new();
            timer.write_state(&mut writer);
            assert_eq!(writer.into_bytes(), bytes);
        }
    }
    for bytes in [[0, 0, 16, 0], [0, 0, 0, 16], [0, 1, 0, 4]] {
        assert!(TimerB::read_state(&mut StateReader::new(&bytes)).is_err());
    }
    for len in 0..4 {
        assert!(TimerB::read_state(&mut StateReader::new(&[0; 4][..len])).is_err());
    }
}

use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn scalar(timer: &mut TimerA, cycle: usize, slots: u64, mode: u8, flags: &mut u8) {
    for offset in 0..slots {
        timer.clock((cycle + offset as usize) % SLOTS, mode, flags);
    }
}

#[test]
fn start_overflow_reload_and_clear_have_distinct_slot_owners() {
    let mut timer = TimerA {
        reload: 1023,
        ..TimerA::default()
    };
    let mut flags = 2;
    timer.clock(0, 0x81, &mut flags);
    timer.clock(1, 0x81, &mut flags);
    assert_eq!(timer.counter, 0);
    assert!(!timer.csm());
    timer.clock(2, 0x81, &mut flags);
    assert!(timer.csm() && timer.reload_pending);
    assert_eq!(flags, 2);
    timer.reload = 1022;
    timer.clock(3, 0x81, &mut flags);
    assert_eq!(timer.counter, 1022);
    assert!(!timer.reload_pending);
    scalar(&mut timer, 4, 22, 0x81, &mut flags);
    assert_eq!(timer.counter, 1023);
    timer.clock(2, 0x81, &mut flags);
    assert!(!timer.csm());
    scalar(&mut timer, 3, 23, 0x81, &mut flags);
    assert!(timer.overflow);
    assert_eq!(flags, 2);
    timer.clear(true);
    timer.clock(2, 0x85, &mut flags);
    assert!(timer.csm() && timer.reload_pending);
    assert_eq!(flags, 2);
    timer.clock(3, 0x85, &mut flags);
    assert_eq!(timer.counter, 1022);
    assert_eq!(flags, 2);
    scalar(&mut timer, 4, 46, 0x85, &mut flags);
    assert!(timer.overflow);
    timer.clock(2, 0x85, &mut flags);
    assert_eq!(flags, 3);
}

#[test]
fn stop_start_between_lock_samples_does_not_create_a_new_start() {
    let mut timer = TimerA {
        reload: 1023,
        ..TimerA::default()
    };
    let mut flags = 0;
    scalar(&mut timer, 0, 4, 0x81, &mut flags);
    assert!(timer.csm());
    scalar(&mut timer, 4, 10, 0x80, &mut flags);
    assert!(timer.csm());
    scalar(&mut timer, 14, 12, 0x81, &mut flags);
    assert!(timer.overflow);
    timer.clock(2, 0xc1, &mut flags);
    assert!(!timer.csm());
    timer.clock(3, 0xc1, &mut flags);
    assert_eq!(timer.counter, 1023);
    assert!(timer.load_lock);
}

#[test]
fn timer_projection_matches_scalar_across_transients_and_scan_cuts() {
    for reload in [0, 511, 1022, 1023] {
        for initial in [0, 5, 0x81, 0x85] {
            for mode in [0, 1, 5, 0x80, 0x81, 0x85, 0xc5] {
                for cut in 0..SLOTS {
                    let mut original = TimerA {
                        reload,
                        ..TimerA::default()
                    };
                    let mut flags = 2;
                    scalar(
                        &mut original,
                        0,
                        (SLOTS * 3 + cut) as u64,
                        initial,
                        &mut flags,
                    );
                    original.clear(cut % 3 == 0);
                    original.reload = 1023 - reload;
                    for slots in [0, 1, 2, 23, 24, 25, 144, 1009] {
                        let mut fast = original;
                        let mut slow = original;
                        let mut a = flags;
                        let mut b = flags;
                        fast.advance_slots(cut, slots, mode, &mut a);
                        scalar(&mut slow, cut, slots, mode, &mut b);
                        assert_eq!(
                            (fast, a),
                            (slow, b),
                            "{reload}/{initial}/{mode}/{cut}/{slots}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn huge_timer_projection_composes_at_every_scan_phase() {
    for reload in [0, 511, 1022, 1023] {
        for mode in [0, 5, 0x80, 0x81, 0x85, 0xc5] {
            for cut in 0..SLOTS {
                let mut whole = TimerA {
                    reload,
                    ..TimerA::default()
                };
                let mut chunks = whole;
                let mut a = 2;
                let mut b = 2;
                whole.advance_slots(cut, u64::MAX, mode, &mut a);
                chunks.advance_slots(cut, u64::MAX - 1007, mode, &mut b);
                let next = (cut as u64 + (u64::MAX - 1007) % SLOTS as u64) % SLOTS as u64;
                chunks.advance_slots(next as usize, 1007, mode, &mut b);
                assert_eq!((whole, a), (chunks, b));
            }
        }
    }
}

#[test]
fn old_countdowns_keep_coarse_progress_without_inventing_csm_or_reload_edges() {
    for remaining in [0, 1, 143, 144, 145, 1024 * 144] {
        for running in [false, true] {
            if remaining == 0 && running {
                continue;
            }
            for version in 1..=11 {
                let mut writer = StateWriter::new();
                writer.write_u16(1022);
                writer.write_u64(remaining);
                writer.write_bool(running);
                let bytes = writer.into_bytes();
                let timer = TimerA::read_state(&mut StateReader::new(&bytes), version).unwrap();
                let scans = remaining.div_ceil(144) as u16;
                assert_eq!(timer.counter, if scans == 0 { 0 } else { 1024 - scans });
                assert_eq!(timer.load_lock, running);
                assert!(!timer.reload_pending && !timer.overflow && !timer.clear && !timer.csm);
                let mut writer = StateWriter::new();
                timer.write_state(&mut writer, crate::save_state::STATE_VERSION);
                let current = writer.into_bytes();
                assert_eq!(
                    TimerA::read_state(
                        &mut StateReader::new(&current),
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
fn timer_codec_rejects_invalid_counters_latches_and_overflow() {
    for bytes in [
        vec![0, 4, 0, 0, 0],
        vec![0, 0, 0, 4, 0],
        vec![0, 0, 0, 0, 32],
        vec![0, 0, 1, 0, 4],
    ] {
        assert!(
            TimerA::read_state(
                &mut StateReader::new(&bytes),
                crate::save_state::STATE_VERSION
            )
            .is_err()
        );
    }
    for len in 0..5 {
        assert!(
            TimerA::read_state(
                &mut StateReader::new(&[0; 5][..len]),
                crate::save_state::STATE_VERSION
            )
            .is_err()
        );
    }
}

use super::*;

fn scalar(chip: &mut Opn2, clocks: u64) {
    chip.advance_timers(clocks);
    let steps = clocks / scheduler::SLOT_CLOCKS
        + (clocks % scheduler::SLOT_CLOCKS + chip.phase % scheduler::SLOT_CLOCKS)
            / scheduler::SLOT_CLOCKS;
    chip.phase = (clocks % FM_CLOCK_DIVIDER + chip.phase) % FM_CLOCK_DIVIDER;
    for _ in 0..steps {
        chip.clock_slot();
    }
}

fn quiet(control: u8, quotient: u8, counter: u8) -> Opn2 {
    let mut chip = Opn2::default();
    for channel in 0..6_u8 {
        let bank = (channel / 3) * 2;
        let local = channel % 3;
        for (address, value) in [
            (0xa4 + local, (channel + 1) << 3 | 3),
            (0xa0 + local, 79 + channel * 13),
            (0xb4 + local, 0xc0 | channel << 4 | channel),
        ] {
            chip.write_port(bank, address);
            chip.write_port(bank + 1, value);
        }
        for slot in 0..4_u8 {
            for (group, value) in [
                (0x30, (channel + slot) << 4 | (channel * 2 + slot)),
                (0x40, channel * 7 + slot),
                (0x50, channel << 6 | 15),
                (0x60, 0x80 | channel),
                (0x80, 0xf0 | channel),
            ] {
                chip.write_port(bank, group + slot * 4 + local);
                chip.write_port(bank + 1, value);
            }
        }
    }
    chip.advance(144 * 4);
    chip.lfo = Lfo {
        control,
        quotient,
        counter,
        sampled: counter,
    };
    chip
}

#[test]
fn silent_fast_forward_preserves_every_stage_with_lfo_and_phase() {
    for control in 0..16 {
        for quotient in [0, 1, 127] {
            let original = quiet(control, quotient, (control * 7 + quotient) & 127);
            for frames in [1, 3, 17, 129, 257] {
                let mut fast = original;
                let mut slow = original;
                fast.advance(frames * 144 + 17);
                scalar(&mut slow, frames * 144 + 17);
                assert_eq!(fast, slow);
            }
        }
    }
}

#[test]
fn huge_silent_advances_compose_without_discarding_pending_wake() {
    for control in [0, 8, 15] {
        let original = quiet(control, 127, 93);
        assert_huge_and_wake(original);
    }
}

fn assert_huge_and_wake(original: Opn2) {
    let mut whole = original;
    let mut chunks = original;
    whole.advance(u64::MAX);
    chunks.advance(u64::MAX - 1007);
    chunks.advance(1007);
    assert_eq!(whole, chunks);
    for chip in [&mut whole, &mut chunks] {
        chip.write_port(0, 0x28);
        chip.write_port(1, 0x10);
    }
    whole.advance(6 * 97 + 3);
    for clocks in [1, 5, 31, 0, 144, 3, 400, 1] {
        chunks.advance(clocks);
    }
    assert_eq!(whole, chunks);
    assert!(whole.slot(0).control_snapshot().2);
}

#[test]
fn silent_phase_wrap_and_negative_detune_cross_the_late_slot_boundary() {
    let mut original = quiet(15, 127, 0);
    for (address, value) in [
        (0x3c, 0x0f),
        (0x3d, 0x7f),
        (0xb4, 0xc7),
        (0xb5, 0xc7),
        (0xa4, 0x3f),
        (0xa0, 0xff),
        (0xa5, 0x38),
        (0xa1, 0),
    ] {
        original.write_port(0, address);
        original.write_port(1, value);
    }
    scalar(&mut original, 144 * 4);
    assert!(original.quiet_at_scan_boundary());
    for slot in [18, 19] {
        let frequency = original.slot_frequency(slot);
        assert!(
            original.slot(slot).modulated_phase_step(
                frequency.0 * 2,
                frequency.1,
                operator::key_code(frequency.0, frequency.1),
            ) > constants::PHASE_MASK
        );
    }
    for counter in [0, 63, 64, 127] {
        original.lfo.counter = counter;
        original.lfo.quotient = 127;
        for frames in [3, 129, 2049] {
            let mut fast = original;
            let mut slow = original;
            fast.advance(frames * 144 + 17);
            scalar(&mut slow, frames * 144 + 17);
            assert_eq!(fast, slow);
        }
    }
    assert_huge_and_wake(original);
}

#[test]
fn silent_special_mode_changes_preserve_phase_and_key_wake() {
    let mut base = quiet(15, 127, 93);
    for (address, value) in [
        (0xad, 0x3f),
        (0xa9, 0xf1),
        (0xac, 0x08),
        (0xa8, 0x01),
        (0xae, 0x27),
        (0xaa, 0x7f),
        (0x3e, 0x7f),
    ] {
        base.write_port(0, address);
        base.write_port(1, value);
    }
    for initial in [0, 0x40, 0x80, 0xc0] {
        for mode in [0, 0x40, 0x80, 0xc0] {
            for cut in 0..24 {
                for fraction in 0..6 {
                    let mut slow = base;
                    slow.write_port(0, 0x27);
                    slow.write_port(1, initial);
                    scalar(&mut slow, 6 * (96 + cut) + fraction);
                    slow.write_port(1, mode);
                    let mut fast = slow;
                    fast.advance(144 * 33 + 17);
                    scalar(&mut slow, 144 * 33 + 17);
                    assert_eq!(fast, slow);
                    for chip in [&mut fast, &mut slow] {
                        chip.write_port(0, 0x28);
                        chip.write_port(1, 0xf2);
                    }
                    fast.advance(144 * 5 + 19);
                    scalar(&mut slow, 144 * 5 + 19);
                    assert_eq!(fast, slow);
                }
            }
        }
    }
}

#[test]
fn migrated_total_level_caches_resample_before_silent_skip_and_key_wake() {
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    for mode in [0, 0x40, 0x80, 0xc0] {
        for version in 1..=10 {
            let mut original = quiet(15, 127, 93);
            if version < crate::save_state::STATE_VERSION_WITH_FM_PIPELINE {
                for channel in 0..6 {
                    let bank = (channel / 3) * 2;
                    original.write_port(bank, 0xb4 + channel % 3);
                    original.write_port(bank + 1, 0xc0);
                }
                original.advance(144 * 4);
            }
            original.write_port(0, 0x27);
            original.write_port(1, mode);
            let mut writer = StateWriter::new();
            original.write_state_version(&mut writer, version);
            let bytes = writer.into_bytes();
            let mut fast =
                Opn2::read_state_version(&mut StateReader::new(&bytes), version).unwrap();
            assert!((0..24).all(|slot| fast.slot(slot).pipeline.total_level == 128));
            assert!(!fast.quiet_at_scan_boundary());
            let mut writer = StateWriter::new();
            fast.write_state(&mut writer);
            let current = writer.into_bytes();
            let mut slow = Opn2::read_state(&mut StateReader::new(&current)).unwrap();
            fast.advance(144 * 33 + 17);
            scalar(&mut slow, 144 * 33 + 17);
            assert_eq!(fast, slow);
            assert!((0..24).all(|slot| fast.slot(slot).pipeline.total_level < 128));
            for chip in [&mut fast, &mut slow] {
                chip.write_port(0, 0x28);
                chip.write_port(1, 0xf2);
            }
            fast.advance(144 * 5 + 19);
            scalar(&mut slow, 144 * 5 + 19);
            assert_eq!(fast, slow);
        }
    }
}

#[test]
fn running_timer_a_silent_skip_preserves_flags_phases_and_key_wake() {
    for mode in [1, 5, 0x41, 0x45, 0xc1, 0xc5] {
        for reload in [0, 1022, 1023] {
            for cut in 0..24 {
                let mut slow = quiet(15, 127, 93);
                for (address, value) in [
                    (0x24, (reload >> 2) as u8),
                    (0x25, reload as u8 & 3),
                    (0x27, mode),
                ] {
                    slow.write_port(0, address);
                    slow.write_port(1, value);
                }
                scalar(&mut slow, cut * 6 + 5);
                let mut fast = slow;
                fast.advance(144 * 65 + 17);
                scalar(&mut slow, 144 * 65 + 17);
                assert_eq!(fast, slow);
                for chip in [&mut fast, &mut slow] {
                    chip.write_port(0, 0x28);
                    chip.write_port(1, 0xf2);
                }
                fast.advance(144 * 5 + 19);
                scalar(&mut slow, 144 * 5 + 19);
                assert_eq!(fast, slow);
                assert_eq!(
                    fast.sample_with_mutes([false; 6]),
                    slow.sample_with_mutes([false; 6])
                );
            }
        }
    }
}

#[test]
fn pending_held_and_sampled_csm_prevent_silent_skip_until_drained() {
    let mut chip = quiet(15, 127, 93);
    assert!(chip.quiet_at_scan_boundary());
    for (address, value) in [(0x24, 0xff), (0x25, 3), (0x27, 0x81)] {
        chip.write_port(0, address);
        chip.write_port(1, value);
    }
    assert!(!chip.quiet_at_scan_boundary());
    chip.advance(18);
    assert!(chip.timer_a.csm());
    assert!(chip.slot(2).pipeline.csm);
    chip.write_port(0, 0x27);
    chip.write_port(1, 0);
    let mut fast = chip;
    let mut slow = chip;
    fast.advance(126);
    scalar(&mut slow, 126);
    assert_eq!(fast, slow);
    assert!(fast.timer_a.csm());
    assert!(!fast.quiet_at_scan_boundary());
    fast.advance(144 * 65);
    scalar(&mut slow, 144 * 65);
    assert_eq!(fast, slow);
    assert!(!fast.timer_a.csm());
    assert!((0..24).all(|slot| !fast.slot(slot).pipeline.csm));
}

#[test]
fn huge_running_timer_projection_matches_quiet_advance_and_preserves_the_source() {
    for mode in [0, 5, 0x45, 0xc5] {
        for reload in [0, 1022, 1023] {
            for fraction in 0..6 {
                let mut source = quiet(15, 127, 93);
                for (address, value) in [
                    (0x24, (reload >> 2) as u8),
                    (0x25, reload as u8 & 3),
                    (0x27, mode),
                ] {
                    source.write_port(0, address);
                    source.write_port(1, value);
                }
                source.advance(fraction);
                let saved = source;
                let status = source.status_after(u64::MAX);
                assert_eq!(source, saved);
                let mut whole = source;
                let mut parts = source;
                whole.advance(u64::MAX);
                parts.advance(u64::MAX - 1007);
                parts.advance(1007);
                assert_eq!(whole, parts);
                assert_eq!(status, whole.status(0));
            }
        }
    }
}

#[test]
fn timer_b_quiet_projection_preserves_stopped_phase_and_future_key_wake() {
    for reload in [0, 1, 254, 255] {
        for mode in [0, 2, 10, 15, 0x4f, 0xcf] {
            for wait in 0..16 {
                let mut slow = quiet(15, 127, 93);
                scalar(&mut slow, wait * 144);
                for (address, value) in [(0x24, 255), (0x25, 3), (0x26, reload), (0x27, mode)] {
                    slow.write_port(0, address);
                    slow.write_port(1, value);
                }
                scalar(&mut slow, wait * 6 + 5);
                slow.write_port(0, 0x27);
                slow.write_port(1, mode | 0x30);
                let mut fast = slow;
                fast.advance(144 * 65 + 17);
                scalar(&mut slow, 144 * 65 + 17);
                assert_eq!(fast, slow);
                for chip in [&mut fast, &mut slow] {
                    chip.write_port(0, 0x27);
                    chip.write_port(1, mode | 10);
                    chip.write_port(0, 0x28);
                    chip.write_port(1, 0xf2);
                }
                fast.advance(144 * 33 + 19);
                scalar(&mut slow, 144 * 33 + 19);
                assert_eq!(fast, slow);
                assert_eq!(
                    fast.sample_with_mutes([false; 6]),
                    slow.sample_with_mutes([false; 6])
                );
            }
        }
    }
}

#[test]
fn huge_both_timer_advances_match_status_projection_and_future_start() {
    for mode in [0, 2, 10, 15, 0x4f, 0xcf] {
        for reload in [0, 1, 254, 255] {
            for fraction in 0..6 {
                let mut source = quiet(15, 127, 93);
                for (address, value) in [(0x24, 255), (0x25, 3), (0x26, reload), (0x27, mode)] {
                    source.write_port(0, address);
                    source.write_port(1, value);
                }
                source.advance(fraction);
                let saved = source;
                let status = source.status_after(u64::MAX);
                assert_eq!(source, saved);
                let mut whole = source;
                let mut parts = source;
                whole.advance(u64::MAX);
                parts.advance(u64::MAX - 1007);
                parts.advance(1007);
                assert_eq!(whole, parts);
                assert_eq!(status, whole.status(0));
                for chip in [&mut whole, &mut parts] {
                    chip.write_port(0, 0x27);
                    chip.write_port(1, 10);
                }
                whole.advance(144 * 33 + 19);
                scalar(&mut parts, 144 * 33 + 19);
                assert_eq!(whole, parts);
            }
        }
    }
}

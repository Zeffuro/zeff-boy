use super::*;

fn write(chip: &mut Opn2, address: u8, value: u8) {
    chip.write_port(0, address);
    chip.write_port(1, value);
}

fn configured(channel: usize, logical: usize) -> Opn2 {
    let mut chip = Opn2::default();
    let bank = if channel >= 3 { 2 } else { 0 };
    let local = channel % 3;
    for (register, value) in [(0x30, 1), (0x50, 31), (0x80, 0xf0)] {
        chip.write_port(
            bank,
            register
                + local as u8
                + (OPERATOR_ORDER.iter().position(|&op| op == logical).unwrap() * 4) as u8,
        );
        chip.write_port(bank + 1, value);
    }
    chip.write_port(bank, 0xa4 + local as u8);
    chip.write_port(bank + 1, 0x2f);
    chip.write_port(bank, 0xa0 + local as u8);
    chip.write_port(bank + 1, 0);
    chip
}

#[test]
fn key_scan_latch_adsr_and_phase_reset_follow_each_slot() {
    for phase in 0..24 {
        for channel in 0..6 {
            for logical in 0..4 {
                for fraction in 0..6 {
                    let mut chip = configured(channel, logical);
                    chip.advance(6 * (96 + phase) + fraction);
                    let slot = channel + logical_offset(logical);
                    let before = chip.slot(slot).control_snapshot();
                    let encoded = (channel % 3 + channel / 3 * 4) as u8;
                    write(&mut chip, 0x28, encoded | 1 << (4 + logical));
                    assert_eq!(chip.slot(slot).control_snapshot(), before);
                    let copy = (channel as u64 + 24 - phase) % 24;
                    let copy = if copy == 0 { 24 } else { copy };
                    let latch = copy
                        + if logical == 0 {
                            24
                        } else {
                            logical_offset(logical) as u64
                        };
                    for call in 0..=latch + 5 {
                        chip.advance(if call == 0 { 6 - fraction } else { 6 });
                        let state = chip.slot(slot).control_snapshot();
                        assert_eq!(state.2, call >= latch + 2);
                        if call == latch + 5 {
                            assert_eq!(state.3, 0);
                        }
                    }
                }
            }
        }
    }
}

fn logical_offset(logical: usize) -> usize {
    [0, 12, 6, 18][logical]
}

#[test]
fn bounded_calendar_preserves_cold_zero_and_old_action_at_wrap() {
    assert_eq!(Scheduler::counter_at(25), None);
    assert_eq!(Scheduler::counter_at(26), Some(0));
    assert_eq!(Scheduler::counter_at(49), Some(0));
    assert_eq!(Scheduler::counter_at(50), None);
    assert_eq!(Scheduler::counter_at(98), Some(1));
    assert_eq!(Scheduler::counter_at(26 + 4095 * 72), Some(4095));
    assert_eq!(Scheduler::counter_at(26 + 4096 * 72), Some(1));
    let mut whole = Scheduler::default();
    whole.advance_clock(u64::MAX);
    let mut split = Scheduler::default();
    split.advance_clock(u64::MAX - 128);
    split.advance_clock(128);
    assert_eq!(whole, split);
    assert!(whole.step < CLOCK_END);
}

#[test]
fn repeated_commands_and_invalid_channel_preserve_latest_latch() {
    let mut chip = configured(0, 0);
    write(&mut chip, 0x28, 0x10);
    write(&mut chip, 0x28, 0x03);
    chip.advance(6 * 96);
    assert!(!chip.slot(0).control_snapshot().2);
    write(&mut chip, 0x28, 0x10);
    chip.advance(6 * 96);
    assert!(chip.slot(0).control_snapshot().2);
    let phase = chip.slot(0).control_snapshot().3;
    write(&mut chip, 0x28, 0x10);
    chip.advance(6);
    assert_eq!(chip.slot(0).control_snapshot().3, phase);
    assert!(chip.scheduler.pending.is_none());
}

#[test]
fn fractional_chunks_keep_all_pending_control_and_sample_inputs() {
    let mut whole = configured(0, 0);
    write(&mut whole, 0x28, 0x10);
    let mut chunks = whole;
    for duration in [1, 5, 6, 7, 143, 144, 145, 2001] {
        whole.advance(duration);
        for _ in 0..duration {
            chunks.advance(1);
        }
        assert_eq!(whole, chunks);
    }
}

fn special_frequencies(chip: &mut Opn2) {
    for (address, value) in [
        (0xad, 0x0b),
        (0xa9, 0x21),
        (0xac, 0x15),
        (0xa8, 0x21),
        (0xae, 0x26),
        (0xaa, 0x21),
    ] {
        write(chip, address, value);
    }
}

#[test]
fn live_special_modes_preserve_the_selected_pitch_and_fractional_restore() {
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    for initial in [0, 0x40, 0x80, 0xc0] {
        for mode in [0, 0x40, 0x80, 0xc0] {
            for cut in 0..24 {
                for fraction in 0..6 {
                    let mut chip = configured(2, 0);
                    special_frequencies(&mut chip);
                    write(&mut chip, 0x27, initial);
                    chip.advance(6 * (96 + cut) + fraction);
                    let cached = chip.scheduler.frequency;
                    let increment = chip.slot(cut as usize).modulated_phase_step(
                        cached.0 * 2,
                        cached.1,
                        operator::key_code(cached.0, cached.1),
                    ) & constants::PHASE_MASK;
                    write(&mut chip, 0x28, 0x12);
                    write(&mut chip, 0x27, mode);
                    assert_eq!(chip.scheduler.frequency, cached);
                    let mut writer = StateWriter::new();
                    chip.write_state(&mut writer);
                    let bytes = writer.into_bytes();
                    let mut restored = Opn2::read_state(&mut StateReader::new(&bytes)).unwrap();
                    assert_eq!(restored, chip);
                    chip.advance(6 - fraction);
                    for _ in 0..6 - fraction {
                        restored.advance(1);
                    }
                    assert_eq!(chip, restored);
                    assert_eq!(chip.slot(cut as usize).pipeline.increment, increment);
                    let next = (cut + 1) as usize % 24;
                    let expected = match (mode, next) {
                        (0x40 | 0x80 | 0xc0, 2) => (0x321, 1),
                        (0x40 | 0x80 | 0xc0, 8) => (0x521, 2),
                        (0x40 | 0x80 | 0xc0, 14) => (0x621, 4),
                        (_, 2 | 8 | 14 | 20) => (0x700, 5),
                        _ => (0, 0),
                    };
                    assert_eq!(chip.scheduler.frequency, expected);
                    chip.advance(1007);
                    for _ in 0..1007 {
                        restored.advance(1);
                    }
                    assert_eq!(chip, restored);
                }
            }
        }
    }
}

#[test]
fn special_mode_old_states_keep_saved_pitch_latches_and_canonical_registers() {
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    for version in 1..=crate::save_state::STATE_VERSION {
        let mut chip = configured(2, 0);
        special_frequencies(&mut chip);
        chip.advance(6 * 122 + 5);
        write(&mut chip, 0x27, 0x80);
        let mut writer = StateWriter::new();
        chip.write_state_version(&mut writer, version);
        let bytes = writer.into_bytes();
        let restored = Opn2::read_state_version(&mut StateReader::new(&bytes), version).unwrap();
        assert_eq!(restored.mode, 0x80);
        assert_eq!(restored.special, chip.special);
        if version >= crate::save_state::STATE_VERSION_WITH_SLOT_SCHEDULER {
            assert_eq!(restored.scheduler.frequency, chip.scheduler.frequency);
        } else {
            assert!(!restored.scheduler.frequency_valid);
        }
        let mut writer = StateWriter::new();
        restored.write_state(&mut writer);
        let current = writer.into_bytes();
        let mut again = Opn2::read_state(&mut StateReader::new(&current)).unwrap();
        let mut restored = restored;
        restored.advance(2017);
        for clocks in [1, 5, 1007, 1004] {
            again.advance(clocks);
        }
        assert_eq!(restored, again);
    }
}

use super::Opn2;

fn write(chip: &mut Opn2, bank: u8, address: u8, value: u8) {
    chip.write_port(bank * 2, address);
    chip.write_port(bank * 2 + 1, value);
}

fn tone(channel: u8) -> Opn2 {
    let mut chip = Opn2::default();
    let bank = channel / 3;
    let index = channel % 3;
    for slot in [0, 4, 8, 12] {
        write(&mut chip, bank, 0x30 + index + slot, 1);
        write(
            &mut chip,
            bank,
            0x40 + index + slot,
            if slot == 12 { 0 } else { 127 },
        );
        write(&mut chip, bank, 0x50 + index + slot, 31);
    }
    write(&mut chip, bank, 0xb0 + index, 7);
    write(&mut chip, bank, 0xb4 + index, 0xc0);
    write(&mut chip, bank, 0xa4 + index, 0x1c);
    write(&mut chip, bank, 0xa0 + index, 0);
    write(&mut chip, 0, 0x28, 0x80 | (bank * 4) | index);
    chip
}

#[test]
fn busy_lasts_192_input_clocks_and_address_writes_do_not_restart_it() {
    let mut chip = Opn2::default();
    chip.write_port(0, 0x2a);
    assert_eq!(chip.status(0), 0);
    chip.write_port(1, 0xff);
    assert_eq!(chip.status(0), 0x80);
    chip.advance(191);
    chip.write_port(2, 0xb6);
    assert_eq!(chip.status(0), 0x80);
    chip.advance(1);
    assert_eq!(chip.status(0), 0);
    chip.write_port(3, 0xc0);
    assert_eq!(chip.status_after(191), 0x80);
    assert_eq!(chip.status_after(192), 0);
    assert_eq!(chip.status(0), 0x80);
    for port in [1, 2, 3] {
        assert_eq!(chip.status(port), 0);
    }
}

#[test]
fn address_is_one_shared_nine_bit_latch_and_globals_are_bank_zero_only() {
    let mut chip = Opn2::default();
    chip.write_port(0, 0x2a);
    chip.write_port(3, 0x91);
    assert_eq!(chip.debug_snapshot().dac, 0x91);
    chip.write_port(2, 0x2a);
    chip.write_port(1, 0x20);
    assert_eq!(chip.debug_snapshot().address, 0x12a);
    assert_eq!(chip.debug_snapshot().dac, 0x91);
    write(&mut chip, 1, 0x27, 0x0f);
    assert_eq!(chip.debug_snapshot().mode, 0);
    chip.write_port(0, 0x25);
    chip.write_port(2, 0x24);
    chip.write_port(1, 0xff);
    assert_eq!(chip.debug_snapshot().timer_a, 0);
}

#[test]
fn timer_a_smallest_period_and_both_reload_halves_are_exact() {
    let mut chip = Opn2::default();
    write(&mut chip, 0, 0x24, 0xff);
    write(&mut chip, 0, 0x25, 0xff);
    assert_eq!(chip.debug_snapshot().timer_a, 1023);
    write(&mut chip, 0, 0x27, 5);
    chip.advance(161);
    assert_eq!(chip.status(0) & 3, 0);
    chip.advance(1);
    assert_eq!(chip.status(0) & 3, 1);
    write(&mut chip, 0, 0x27, 0x15);
    assert_eq!(chip.status(0) & 3, 1);
    chip.advance(143);
    assert_eq!(chip.status(0) & 3, 0);
    chip.advance(1);
    assert_eq!(chip.status(0) & 3, 1);
}

#[test]
fn timer_b_minimum_and_independent_overflow_flags() {
    let mut chip = Opn2::default();
    write(&mut chip, 0, 0x24, 0xfc);
    write(&mut chip, 0, 0x25, 0);
    write(&mut chip, 0, 0x26, 0xff);
    write(&mut chip, 0, 0x27, 0x0f);
    chip.advance(2177);
    assert_eq!(chip.status(0), 0);
    chip.advance(1);
    assert_eq!(chip.status(0), 2);
    chip.advance(144);
    assert_eq!(chip.status(0), 3);
    write(&mut chip, 0, 0x27, 0x10);
    chip.advance(6);
    assert_eq!(chip.status(0) & 3, 2);
    chip.advance(10_000);
    assert_eq!(chip.status(0), 2);
    write(&mut chip, 0, 0x27, 0x20);
    assert_eq!(chip.status(0) & 3, 2);
    chip.advance(6);
    assert_eq!(chip.status(0) & 3, 0);
}

#[test]
fn timer_enable_gates_flags_and_live_reload_waits_for_current_period() {
    let mut chip = Opn2::default();
    write(&mut chip, 0, 0x24, 0xff);
    write(&mut chip, 0, 0x25, 0);
    write(&mut chip, 0, 0x27, 1);
    chip.advance(700);
    assert_eq!(chip.status(0), 0);
    write(&mut chip, 0, 0x27, 5);
    chip.advance(469);
    assert_eq!(chip.status(0) & 3, 0);
    chip.advance(1);
    assert_eq!(chip.status(0) & 3, 1);
    write(&mut chip, 0, 0x27, 0x15);
    chip.advance(100);
    write(&mut chip, 0, 0x25, 3);
    chip.advance(475);
    assert_eq!(chip.status(0) & 3, 0);
    chip.advance(1);
    assert_eq!(chip.status(0) & 3, 1);
    write(&mut chip, 0, 0x27, 0x15);
    chip.advance(143);
    assert_eq!(chip.status(0) & 3, 0);
    chip.advance(1);
    assert_eq!(chip.status(0) & 3, 1);
}

#[test]
fn stopped_timer_stays_stopped_and_restart_loads_the_current_reload() {
    let mut chip = Opn2::default();
    write(&mut chip, 0, 0x24, 0xff);
    write(&mut chip, 0, 0x25, 3);
    write(&mut chip, 0, 0x27, 5);
    chip.advance(100);
    write(&mut chip, 0, 0x27, 4);
    chip.advance(62);
    assert_eq!(chip.status(0) & 1, 1);
    write(&mut chip, 0, 0x27, 0x14);
    chip.advance(6);
    chip.advance(144 * 10 - 168);
    assert_eq!(chip.status(0), 0);
    write(&mut chip, 0, 0x27, 5);
    chip.advance(161);
    assert_eq!(chip.status(0) & 3, 0);
    chip.advance(1);
    assert_eq!(chip.status(0) & 3, 1);
}

#[test]
fn timer_and_busy_status_prediction_is_nonmutating_and_partition_independent() {
    let mut chip = Opn2::default();
    write(&mut chip, 0, 0x24, 0xfd);
    write(&mut chip, 0, 0x25, 2);
    write(&mut chip, 0, 0x26, 0xf9);
    write(&mut chip, 0, 0x27, 0x0f);
    let saved = chip;
    assert_eq!(chip.status_after(20_000), 3);
    assert_eq!(chip, saved);
    let mut parts = chip;
    chip.advance(20_000);
    for clocks in [1, 143, 57, 2304, 17_495] {
        parts.advance(clocks);
    }
    assert_eq!(chip, parts);
    chip.advance(u64::MAX - 20_000);
    assert_eq!(chip.status(0), 3);
}

#[test]
fn all_six_channels_synthesize_through_both_register_banks_and_key_codes() {
    for channel in 0..6 {
        let mut chip = tone(channel);
        chip.advance(144 * 65);
        let snapshot = chip.debug_snapshot();
        let held = snapshot.output[usize::from(channel)];
        assert!((32..=8160).contains(&held));
        assert_eq!(held % 32, 0);
        for (index, value) in snapshot.output.into_iter().enumerate() {
            assert_eq!(
                value,
                if index == usize::from(channel) {
                    held
                } else {
                    0
                }
            );
        }
        assert_eq!(chip.sample_with_mutes([false; 6]), [held; 2]);
        let mut mutes = [false; 6];
        mutes[usize::from(channel)] = true;
        assert_eq!(chip.sample_with_mutes(mutes), [0, 0]);
    }
}

#[test]
fn frequency_high_write_latches_until_low_write_and_native_phase_is_partitioned() {
    let mut original = tone(0);
    let mut pending = original;
    write(&mut pending, 0, 0xa4, 0x24);
    original.advance(144 * 65);
    pending.advance(144 * 65);
    assert_eq!(
        original.debug_snapshot().output,
        pending.debug_snapshot().output
    );
    write(&mut pending, 0, 0xa0, 0);
    let mut parts = pending;
    pending.advance(144 * 137 + 17);
    for clocks in [1, 143, 17, 144 * 136] {
        parts.advance(clocks);
    }
    assert_eq!(pending, parts);
    original.advance(144 * 137 + 17);
    assert_ne!(
        original.debug_snapshot().output,
        pending.debug_snapshot().output
    );
}

#[test]
fn power_on_dac_plays_on_both_outputs_without_a_pan_write() {
    let mut chip = Opn2::default();
    assert_eq!(chip.sample_with_mutes([false; 6]), [0, 0]);
    write(&mut chip, 0, 0x2b, 0x80);
    for (value, expected) in [(0, -8416), (255, 8128), (128, 0)] {
        write(&mut chip, 0, 0x2a, value);
        assert_eq!(chip.sample_with_mutes([false; 6]), [expected; 2]);
    }
    write(&mut chip, 1, 0xb6, 0);
    write(&mut chip, 0, 0x2a, 255);
    assert_eq!(chip.sample_with_mutes([false; 6]), [0, 0]);
}

#[test]
fn dac_center_extremes_pan_mute_and_fm_replacement() {
    let mut chip = tone(5);
    chip.advance(144 * 65);
    let fm = chip.sample_with_mutes([false; 6]);
    assert_eq!(fm, [chip.output[5]; 2]);
    assert!(fm[0] > 0);
    write(&mut chip, 0, 0x2b, 0x80);
    assert_eq!(chip.sample_with_mutes([false; 6]), [0, 0]);
    for (value, expected) in [(0_u8, -8416), (255, 8128), (128, 0)] {
        write(&mut chip, 0, 0x2a, value);
        let disabled = if expected < 0 { -256 } else { 0 };
        for (pan, output) in [
            (0, [disabled; 2]),
            (0x80, [expected, disabled]),
            (0x40, [disabled, expected]),
            (0xc0, [expected; 2]),
        ] {
            write(&mut chip, 1, 0xb6, pan);
            assert_eq!(chip.sample_with_mutes([false; 6]), output);
        }
    }
    write(&mut chip, 0, 0x2a, 255);
    let mut mutes = [false; 6];
    mutes[5] = true;
    assert_eq!(chip.sample_with_mutes(mutes), [0, 0]);
    write(&mut chip, 0, 0x2b, 0);
    assert_eq!(chip.sample_with_mutes([false; 6]), fm);
}

#[test]
fn dac_override_and_mute_preserve_fm_phase_for_reenable() {
    let mut ordinary = tone(5);
    let mut overridden = ordinary;
    write(&mut overridden, 0, 0x2b, 0x80);
    for clocks in [1, 143, 1000, 19_003] {
        ordinary.advance(clocks);
        overridden.advance(clocks);
        assert_eq!(
            ordinary.debug_snapshot().output,
            overridden.debug_snapshot().output
        );
        assert_eq!(overridden.sample_with_mutes([true; 6]), [0, 0]);
    }
    write(&mut overridden, 0, 0x2b, 0);
    assert_eq!(
        ordinary.sample_with_mutes([false; 6]),
        overridden.sample_with_mutes([false; 6])
    );
}

#[test]
fn negative_fm_pan_gate_leaks_but_frontend_mute_is_exact_silence() {
    let mut chip = Opn2::default();
    chip.output[0] = -8192;
    for (pan, expected) in [
        (0xc0, [-8416, -8416]),
        (0x80, [-8416, -256]),
        (0x40, [-256, -8416]),
        (0, [-256, -256]),
    ] {
        write(&mut chip, 0, 0xb4, pan);
        chip.scheduler.output[0] = -8192;
        chip.publish_slot(12);
        assert_eq!(chip.sample_with_mutes([false; 6]), expected);
        assert_eq!(chip.sample_with_mutes([true; 6]), [0; 2]);
    }
}

#[test]
fn previous_full_precision_output_state_is_readable_and_bounded_at_the_dac() {
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    let chip = Opn2 {
        output: [32764, -32764, 31, -1, 0, 0],
        ..Opn2::default()
    };
    let mut writer = StateWriter::new();
    chip.write_state_version(&mut writer, 8);
    let bytes = writer.into_bytes();
    let mut restored = Opn2::read_state_version(&mut StateReader::new(&bytes), 8).unwrap();
    assert_eq!(restored.output, chip.output);
    assert_eq!(restored.sample_with_mutes([false; 6]), [-512; 2]);
    let migrated = restored;
    restored.advance(0);
    assert_eq!(restored, migrated);
    restored.advance(5);
    assert_eq!(restored.sample_with_mutes([false; 6]), [-512; 2]);
    restored.advance(144 * 3);
    assert_eq!(restored.sample_with_mutes([false; 6]), [0; 2]);
}

#[test]
fn release_silence_and_noninstant_rekey_are_partition_independent() {
    let mut whole = tone(0);
    write(&mut whole, 0, 0x8c, 15);
    whole.advance(144 * 79);
    write(&mut whole, 0, 0x28, 0);
    let mut split = whole;
    whole.advance(144 * 5000 + 17);
    for _ in 0..5000 {
        split.advance(144);
    }
    split.advance(17);
    assert!(!whole.channels.iter().any(super::voice::Channel::active));
    assert_eq!(whole, split);
    for chip in [&mut whole, &mut split] {
        write(chip, 0, 0x5c, 20);
        write(chip, 0, 0x28, 0x80);
    }
    let mut audible = false;
    for _ in 0..4096 {
        whole.advance(144);
        split.advance(71);
        split.advance(73);
        let sample = whole.sample_with_mutes([false; 6]);
        audible |= sample != [0, 0];
        assert_eq!(sample, split.sample_with_mutes([false; 6]));
    }
    assert!(audible);
    assert_eq!(whole, split);
}

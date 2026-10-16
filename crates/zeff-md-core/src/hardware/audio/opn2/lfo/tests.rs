use super::*;
use crate::hardware::audio::opn2::{Opn2, operator::Operator};
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[test]
fn phase_modulation_vectors_preserve_truncation_half_bit_and_wrap() {
    for (fnum, block, pms, counter, expected_fnum, base) in [
        (0x240, 1, 6, 12, 0x48d, 582),
        (0x240, 1, 6, 76, 0x473, 569),
        (0x0b0, 3, 3, 24, 0x161, 706),
        (0x0b0, 3, 3, 88, 0x15f, 702),
        (0x0b0, 3, 6, 12, 0x163, 710),
        (0x0b0, 3, 7, 12, 0x167, 718),
        (0x7ff, 0, 7, 28, 0x0bc, 47),
        (0x7ff, 0, 7, 92, 0xf40, 976),
        (0x010, 2, 7, 28, 0x021, 33),
        (0x011, 1, 7, 92, 0x021, 16),
        (0x3ff, 3, 7, 28, 0x85c, 4280),
        (0x00f, 7, 7, 28, 0x01e, 960),
    ] {
        assert_eq!(frequency(fnum, counter, pms), expected_fnum);
        let mut operator = Operator::default();
        operator.write(3, 1);
        assert_eq!(operator.modulated_phase_step(expected_fnum, block, 0), base);
    }
}

#[test]
fn am_waveform_covers_all_depths_and_operator_enable() {
    for (counter, expected) in [
        (0, [0, 15, 63, 126]),
        (63, [0, 0, 0, 0]),
        (64, [0, 0, 0, 0]),
        (127, [0, 15, 63, 126]),
    ] {
        for (ams, level) in expected.into_iter().enumerate() {
            assert_eq!(amplitude(counter, ams as u8), level);
        }
    }
    let mut plain = Operator::default();
    plain.write(3, 1);
    plain.write(5, 31);
    plain.key(true, (0x400, 3));
    let mut enabled = plain;
    enabled.write(6, 128);
    let mut muted_at_peak = false;
    for _ in 0..128 {
        let normal = plain.clock_with_lfo((0x400, 3), 0, 0x800, 126);
        let modulated = enabled.clock_with_lfo((0x400, 3), 0, 0x800, 126);
        muted_at_peak |= normal.abs() > 1000 && modulated.abs() < normal.abs() / 3;
    }
    assert!(muted_at_peak);
}

#[test]
fn divider_repeated_writes_speed_changes_and_disabled_clock_match_boundaries() {
    let mut lfo = Lfo::default();
    lfo.write(15);
    for quotient in [1, 2, 3, 4, 5, 1] {
        lfo.write(15);
        lfo.clock();
        assert_eq!(lfo.quotient, quotient);
    }
    assert_eq!(lfo.counter, 1);
    for (quotient, control, ticks) in [(10, 15, 4), (80, 13, 29)] {
        lfo.quotient = quotient;
        lfo.counter = 10;
        lfo.write(control);
        for _ in 1..ticks {
            lfo.clock();
            assert_eq!(lfo.counter, 10);
        }
        lfo.clock();
        assert_eq!(lfo.counter, 11);
        assert_eq!(lfo.quotient, 1);
    }
    lfo.write(0);
    assert_eq!(lfo.counter, 11);
    lfo.clock();
    assert_eq!(lfo.counter, 0);
    assert_eq!(lfo.quotient, 2);
    assert_eq!(amplitude(lfo.counter, 3), 126);
}

#[test]
fn idle_advance_and_split_clocks_preserve_global_lfo() {
    for control in 0..16 {
        for quotient in [0, 1, 4, 10, 80, 108, 127] {
            for clocks in [0, 1, 5, 44, 109, 1000] {
                let mut bulk = Lfo {
                    control,
                    quotient,
                    counter: 33,
                    sampled: 0,
                };
                let mut single = bulk;
                bulk.advance(clocks);
                for _ in 0..clocks {
                    single.clock();
                }
                assert_eq!(bulk, single);
            }
        }
    }
    let mut whole = Opn2::default();
    whole.write_port(0, 0x22);
    whole.write_port(1, 13);
    let mut split = whole;
    whole.advance(144 * 731 + 91);
    for clocks in [1, 144 * 300, 47, 144 * 431 + 43] {
        split.advance(clocks);
    }
    assert_eq!(whole, split);
    assert_ne!(whole.lfo.counter, 0);
    let mut writer = StateWriter::new();
    whole.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut restored = Opn2::read_state(&mut StateReader::new(&bytes)).unwrap();
    for _ in 0..300 {
        whole.advance(144);
        restored.advance(144);
        assert_eq!(whole, restored);
    }
}

#[test]
fn active_lfo_voice_pcm_continues_across_writes_split_clocks_and_state() {
    let mut whole = Opn2::default();
    for (address, value) in [
        (0x30, 1),
        (0x50, 31),
        (0x60, 128),
        (0xb0, 7),
        (0xb4, 0xf7),
        (0xa4, 0x1b),
        (0xa0, 0x7f),
        (0x22, 13),
        (0x28, 0x10),
    ] {
        whole.write_port(0, address);
        whole.write_port(1, value);
    }
    whole.advance(144 * 79 + 39);
    whole.write_port(0, 0x22);
    whole.write_port(1, 15);
    let mut split = whole;
    let mut writer = StateWriter::new();
    whole.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut restored = Opn2::read_state(&mut StateReader::new(&bytes)).unwrap();
    let mut output = Vec::new();
    for _ in 0..1024 {
        whole.advance(144);
        for clocks in [7, 61, 76] {
            split.advance(clocks);
        }
        restored.advance(144);
        output.push(whole.sample_with_mutes([false; 6]));
        assert_eq!(whole, split);
        assert_eq!(whole, restored);
    }
    assert!(output.iter().any(|&sample| sample != [0, 0]));
}

fn control(chip: &mut Opn2, value: u8) {
    chip.write_port(0, 0x22);
    chip.write_port(1, value);
}

#[test]
fn live_counter_and_scan_latch_survive_mid_scan_speed_changes() {
    let mut chip = Opn2::default();
    control(&mut chip, 13);
    chip.advance(144 * 176 + 6);
    assert_eq!(
        (chip.lfo.sampled, chip.lfo.counter, chip.lfo.quotient),
        (3, 4, 0)
    );
    control(&mut chip, 8);
    chip.advance(144);
    assert_eq!(chip.lfo.sampled, 4);
    assert_eq!(amplitude(chip.lfo.sampled, 1), 14);

    let mut chip = Opn2::default();
    control(&mut chip, 8);
    chip.advance(144 * 44 + 6 * 23);
    control(&mut chip, 13);
    chip.advance(6);
    assert_eq!((chip.lfo.counter, chip.lfo.quotient), (1, 0));
    chip.advance(6);
    assert_eq!(
        (chip.lfo.sampled, chip.lfo.counter, chip.lfo.quotient),
        (1, 1, 0)
    );
}

#[test]
fn brief_disable_clears_live_counter_without_changing_the_current_scan_latch() {
    let mut chip = Opn2::default();
    control(&mut chip, 15);
    chip.advance(144 * 12 + 6 * 6);
    assert_eq!(chip.lfo.sampled, 2);
    control(&mut chip, 7);
    chip.advance(6 * 12);
    assert_eq!((chip.lfo.sampled, chip.lfo.counter), (2, 0));
    control(&mut chip, 15);
    chip.advance(6 * 7);
    assert_eq!(chip.lfo.sampled, 0);
    assert_eq!(amplitude(chip.lfo.sampled, 3), 126);
}

#[test]
fn state_preserves_distinct_live_and_sampled_counters_and_seeds_legacy_latches() {
    let mut chip = Opn2::default();
    control(&mut chip, 13);
    chip.advance(144 * 176 + 6);
    let mut writer = StateWriter::new();
    chip.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut reader = StateReader::new(&bytes);
    let mut restored = Opn2::read_state(&mut reader).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(chip, restored);
    for _ in 0..288 {
        chip.advance(1);
        restored.advance(1);
        assert_eq!(chip, restored);
    }
    for version in 1..=7 {
        let mut writer = StateWriter::new();
        chip.write_state_version(&mut writer, version);
        let bytes = writer.into_bytes();
        let restored = Opn2::read_state_version(&mut StateReader::new(&bytes), version).unwrap();
        assert_eq!(restored.lfo.sampled, restored.lfo.counter);
        if version >= 3 {
            assert_eq!(restored.lfo.control, chip.lfo.control);
            assert_eq!(restored.lfo.counter, chip.lfo.counter);
            assert_eq!(restored.lfo.quotient, chip.lfo.quotient);
        }
        if version == 7 {
            let mut writer = StateWriter::new();
            restored.write_state_version(&mut writer, version);
            assert_eq!(writer.into_bytes(), bytes);
        }
    }
    for invalid in [128, 255] {
        let mut bad = bytes.clone();
        let offset = bad.len() - 7;
        bad[offset] = invalid;
        assert!(Opn2::read_state(&mut StateReader::new(&bad)).is_err());
    }
    assert!(Opn2::read_state(&mut StateReader::new(&bytes[..bytes.len() - 1])).is_err());
}

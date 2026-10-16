use super::*;
use crate::hardware::audio::opn2::{Opn2, voice::Channel};
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn operator(detune_multiple: u8) -> Operator {
    let mut operator = Operator::default();
    operator.write(3, detune_multiple);
    operator.write(5, 31);
    operator.key(true, (0, 0));
    operator
}

#[test]
fn key_code_matches_note_bits_for_every_frequency_and_block() {
    for block in 0..8 {
        for fnum in 0..=0x7ff {
            let high = (fnum >> 10) & 1;
            let middle = (fnum >> 7) & 7;
            let low = if high == 0 {
                u8::from(middle == 7)
            } else {
                u8::from(middle != 0)
            };
            assert_eq!(key_code(fnum, block), block * 4 + (high as u8) * 2 + low);
        }
    }
}

#[test]
fn phase_increment_vectors_cover_detune_sign_precision_and_both_wraps() {
    for (fnum, block, dt_mul, expected_key, product, phase) in [
        (0x000, 0, 0x51, 0x00, 0x00000, 0x00000),
        (0x000, 0, 0x61, 0x00, 0x1ffff, 0x1ffff),
        (0x000, 0, 0x60, 0x00, 0x0ffff, 0x0ffff),
        (0x000, 0, 0x6f, 0x00, 0x1dfff1, 0xdfff1),
        (0x000, 0, 0x71, 0x00, 0x1fffe, 0x1fffe),
        (0x000, 1, 0x51, 0x04, 0x1ffff, 0x1ffff),
        (0x000, 7, 0x7f, 0x1c, 0x1dfeb6, 0xdfeb6),
        (0x001, 0, 0x0f, 0x00, 0x00000, 0x00000),
        (0x003, 0, 0x0f, 0x00, 0x0000f, 0x0000f),
        (0x003, 0, 0x20, 0x00, 0x00001, 0x00001),
        (0x003, 0, 0x61, 0x00, 0x00000, 0x00000),
        (0x100, 5, 0x31, 0x14, 0x0100b, 0x0100b),
        (0x100, 5, 0x71, 0x14, 0x00ff5, 0x00ff5),
        (0x100, 5, 0x70, 0x14, 0x007fa, 0x007fa),
        (0x1ff, 4, 0x08, 0x10, 0x07fc0, 0x07fc0),
        (0x1ff, 4, 0x00, 0x10, 0x007fc, 0x007fc),
        (0x1ff, 3, 0x31, 0x0c, 0x00801, 0x00801),
        (0x200, 3, 0x31, 0x0c, 0x00805, 0x00805),
        (0x37f, 3, 0x31, 0x0c, 0x00e01, 0x00e01),
        (0x380, 3, 0x31, 0x0d, 0x00e06, 0x00e06),
        (0x3ff, 3, 0x31, 0x0d, 0x01002, 0x01002),
        (0x400, 3, 0x31, 0x0e, 0x01006, 0x01006),
        (0x47f, 3, 0x31, 0x0e, 0x01202, 0x01202),
        (0x480, 3, 0x31, 0x0f, 0x01207, 0x01207),
        (0x5ff, 3, 0x31, 0x0f, 0x01803, 0x01803),
        (0x600, 3, 0x31, 0x0f, 0x01807, 0x01807),
        (0x7ff, 3, 0x31, 0x0f, 0x02003, 0x02003),
        (0x7ff, 7, 0x3f, 0x1f, 0x1dfd8a, 0xdfd8a),
        (0x7ff, 7, 0x4f, 0x1f, 0x1dfc40, 0xdfc40),
    ] {
        let mut operator = operator(dt_mul);
        assert_eq!(key_code(fnum, block), expected_key);
        assert_eq!(operator.phase_step(fnum, block, expected_key), product);
        operator.clock((fnum, block), 0);
        assert_eq!(operator.phase, phase);
    }
    let mut wrapping = operator(0x61);
    wrapping.phase = 0xffff0;
    wrapping.clock((0, 0), 0);
    assert_eq!(wrapping.phase, 0x1ffef);
}

#[test]
fn detune_negative_zero_preserves_phase_and_wave_at_every_key_code() {
    for block in 0..8 {
        for fnum in [0x100, 0x380, 0x400, 0x480] {
            let mut positive = operator(0x0f);
            let mut negative = operator(0x4f);
            for _ in 0..128 {
                assert_eq!(
                    positive.clock((fnum, block), 0),
                    negative.clock((fnum, block), 0)
                );
                assert_eq!(positive.phase, negative.phase);
            }
        }
    }
}

#[test]
fn zero_frequency_negative_detune_changes_dc_only_with_nonzero_magnitude() {
    let mut silent = operator(0x51);
    let mut audible = operator(0x61);
    let mut nonzero = 0;
    let mut bounds = (i32::MAX, i32::MIN);
    for _ in 0..1024 {
        assert_eq!(silent.clock((0, 0), 0), 25);
        let sample = audible.clock((0, 0), 0);
        nonzero += usize::from(sample != 0);
        bounds = (bounds.0.min(sample), bounds.1.max(sample));
    }
    assert!(nonzero > 900);
    assert!(bounds.0 < 0 && bounds.1 > 0);
    assert_eq!(silent.phase, 0);
    assert_ne!(audible.phase, 0);
}

#[test]
fn zero_frequency_detune_underflow_modulates_another_operator() {
    let mut silent = Channel::default();
    silent.write_channel(0xb0, 6);
    silent.write_operator(0x30, 0x51);
    silent.write_operator(0x38, 1);
    silent.write_operator(0x50, 31);
    silent.write_operator(0x58, 31);
    let mut audible = silent;
    audible.write_operator(0x30, 0x61);
    silent.key(3);
    audible.key(3);
    let mut nonzero = false;
    for _ in 0..1024 {
        assert_eq!(silent.clock(None), 608);
        nonzero |= audible.clock(None) != 608;
    }
    assert!(nonzero);
}

#[test]
fn detune_underflow_continues_exactly_across_split_clocks_and_saved_state() {
    let mut whole = Opn2::default();
    for (address, value) in [(0x3c, 0x61), (0x5c, 31), (0xb0, 7), (0x28, 0x80)] {
        whole.write_port(0, address);
        whole.write_port(1, value);
    }
    whole.advance(144 * 7 + 29);
    let mut split = whole;
    let mut writer = StateWriter::new();
    whole.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut restored = Opn2::read_state(&mut StateReader::new(&bytes)).unwrap();
    let mut audible = false;
    for _ in 0..1024 {
        whole.advance(144);
        for clocks in [1, 28, 115] {
            split.advance(clocks);
        }
        restored.advance(144);
        let sample = whole.sample_with_mutes([false; 6]);
        audible |= sample != [0, 0];
        assert_eq!(sample, split.sample_with_mutes([false; 6]));
        assert_eq!(sample, restored.sample_with_mutes([false; 6]));
    }
    assert!(audible);
    assert_eq!(whole, split);
    assert_eq!(whole, restored);
}

#[test]
fn lfo_frequency_modulation_preserves_the_raw_detune_key_code() {
    let mut operator = operator(0x31);
    let adjusted = crate::hardware::audio::opn2::lfo::frequency(0x37f, 28, 7);
    assert_eq!(adjusted, 0x750);
    operator.clock_with_lfo((0x37f, 3), 0, adjusted, 0);
    assert_eq!(operator.phase, 3749);
}

#[test]
fn logarithmic_operator_matches_phase_and_attenuation_boundaries() {
    for (phase, expected) in [
        (0, 25),
        (1, 75),
        (2, 125),
        (127, 5760),
        (128, 5792),
        (247, 8148),
        (248, 8168),
        (255, 8168),
        (256, 8168),
        (263, 8168),
        (264, 8148),
        (383, 5792),
        (510, 75),
        (511, 25),
        (512, -25),
        (513, -75),
        (767, -8168),
        (768, -8168),
        (1023, -25),
        (1024, 25),
    ] {
        assert_eq!(operator(1).clock((0, 0), phase), expected, "phase {phase}");
    }
    for (level, expected) in [
        (0, 8168),
        (1, 8080),
        (7, 7572),
        (8, 7492),
        (63, 4128),
        (64, 4084),
        (127, 2064),
        (128, 2042),
        (255, 516),
        (256, 510),
        (511, 32),
        (512, 31),
        (831, 1),
        (832, 0),
        (1023, 0),
    ] {
        let mut wave = operator(1);
        wave.attenuation = level;
        assert_eq!(wave.clock((0, 0), 255), expected, "level {level}");
        assert_eq!(wave.clock((0, 0), 767), -expected, "level {level}");
    }
}

#[test]
fn logarithmic_operator_combines_tl_am_and_keeps_silent_phase_running() {
    let mut wave = operator(1);
    wave.attenuation = 64;
    wave.write(4, 8);
    wave.write(6, 0x80);
    assert_eq!(wave.clock_with_lfo((0, 0), 255, 0, 64), 1021);
    wave.write(6, 0);
    assert_eq!(wave.clock_with_lfo((0, 0), 255, 0, 64), 2042);
    wave.attenuation = 832;
    let phase = wave.phase;
    assert_eq!(wave.clock((1024, 3), 0), 0);
    assert_eq!(wave.phase, phase + 4096);
    assert!(wave.active());
}

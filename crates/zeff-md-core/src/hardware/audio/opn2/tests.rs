use super::constants::{ENVELOPE_MAX, PHASE_BITS};
use super::envelope_clock::EnvelopeClock;
use super::operator::Operator;
use super::tables::output;
use super::voice::Channel;

fn tone(algorithm: u8) -> Channel {
    let mut channel = Channel::default();
    channel.set_frequency(1024, 3);
    channel.write_channel(0xb0, algorithm);
    channel.write_channel(0xb4, 0xc0);
    for offset in [0, 4, 8, 12] {
        channel.write_operator(0x30 + offset, 1);
        channel.write_operator(0x50 + offset, 31);
        channel.write_operator(0x80 + offset, 15);
    }
    channel
}

#[test]
fn default_is_silent_with_both_outputs_enabled() {
    let mut channel = Channel::default();
    assert!(!channel.active());
    assert_eq!(channel.pan(), 0xc0);
    for _ in 0..1024 {
        assert_eq!(channel.clock(None), 0);
    }
    channel.write_channel(0xb4, 0);
    assert_eq!(channel.pan(), 0);
}

#[test]
fn logarithmic_output_is_antisymmetric_and_attenuation_is_monotonic() {
    for index in 0..512 {
        assert_eq!(
            output(index << (PHASE_BITS - 10), 0),
            -output((index + 512) << (PHASE_BITS - 10), 0)
        );
    }
    let peak = 255 << (PHASE_BITS - 10);
    assert_eq!(output(peak, 0), 8168);
    assert_eq!(output(peak, 64), 4084);
    assert_eq!(output(peak, ENVELOPE_MAX), 0);
    for level in 1..=ENVELOPE_MAX {
        assert!(output(peak, level) <= output(peak, level - 1));
    }
}

#[test]
fn quiet_operator_truncates_magnitude_before_applying_sign() {
    let mut operator = Operator::default();
    operator.write(5, 31);
    operator.write(4, 96);
    operator.key(true, (0, 0));
    for (index, expected) in [
        (4, 0),
        (64, 0),
        (84, 0),
        (508, 0),
        (88, 1),
        (128, 1),
        (256, 1),
        (516, 0),
        (576, 0),
        (596, 0),
        (1020, 0),
        (600, -1),
        (640, -1),
        (768, -1),
    ] {
        assert_eq!(operator.clock((0, 0), index), expected);
    }
}

#[test]
fn carrier_frequency_is_native_clock_fnum_block_and_multiple() {
    for (multiple, expected) in [(0, 4), (1, 8), (2, 16)] {
        let mut channel = tone(7);
        channel.write_operator(0x3c, multiple);
        channel.key(8);
        let mut rises = 0;
        let mut previous = 0;
        for _ in 0..2049 {
            let sample = channel.clock(None);
            rises += usize::from(previous < 0 && sample >= 0);
            previous = sample;
        }
        assert_eq!(rises, expected);
    }
}

#[test]
fn operator_register_order_and_individual_key_bits_agree() {
    for (slot, offset) in [(0, 0), (1, 8), (2, 4), (3, 12)] {
        let mut channel = tone(7);
        channel.write_operator(0x40 + offset, 127);
        channel.key(1 << slot);
        for _ in 0..512 {
            assert_eq!(channel.clock(None), 0);
        }
        channel.write_operator(0x40 + offset, 0);
        assert!((0..512).any(|_| channel.clock(None) != 0));
    }
}

#[test]
fn summed_modulators_feed_s3_and_preserve_individual_middle_key_bits() {
    let mut summed = tone(1);
    summed.write_operator(0x38, 3);
    summed.write_operator(0x34, 5);
    summed.write_operator(0x4c, 24);
    let mut serial = summed;
    serial.write_channel(0xb0, 4);
    summed.key(12);
    serial.key(12);
    let mut plain = tone(7);
    plain.write_operator(0x4c, 24);
    plain.key(8);
    let mut rich = false;
    for _ in 0..512 {
        let sample = summed.clock(None);
        assert_eq!(sample, serial.clock(None));
        rich |= sample != plain.clock(None);
    }
    assert!(rich);
    let mut summed = tone(1);
    summed.write_operator(0x38, 3);
    summed.write_operator(0x34, 5);
    summed.write_operator(0x4c, 24);
    summed.key(10);
    let mut plain = tone(7);
    plain.write_operator(0x4c, 24);
    plain.key(8);
    for _ in 0..512 {
        assert_eq!(summed.clock(None), plain.clock(None));
    }
}

#[test]
fn operator_pipeline_history_survives_channel_state() {
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    for algorithm in [0, 1, 2, 3, 5] {
        let mut channel = tone(algorithm);
        channel.write_operator(0x38, 3);
        channel.write_operator(0x34, 5);
        channel.key(15);
        for _ in 0..79 {
            channel.clock(None);
        }
        let mut writer = StateWriter::new();
        channel.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut restored = Channel::read_state(
            &mut StateReader::new(&bytes),
            crate::save_state::STATE_VERSION,
        )
        .unwrap();
        assert_eq!(restored, channel);
        for _ in 0..512 {
            assert_eq!(restored.clock(None), channel.clock(None));
        }
        assert_eq!(restored, channel);
    }
}

#[test]
fn repeated_key_on_preserves_phase_and_release_finishes() {
    let mut clock = EnvelopeClock::default();
    let mut channel = tone(7);
    channel.key(8);
    for _ in 0..79 {
        channel.clock(None);
    }
    let mut copy = channel;
    channel.key(8);
    assert_eq!(channel, copy);
    for _ in 0..100 {
        assert_eq!(channel.clock(None), copy.clock(None));
    }
    channel.key(0);
    for _ in 0..1024 {
        channel.clock_with_lfo(None, 0, clock.clock());
    }
    assert!(!channel.active());
    assert_eq!(channel.clock(None), 0);
    channel.key(8);
    assert!(channel.active());
    assert!((0..256).any(|_| channel.clock(None) != 0));
}

#[test]
fn every_algorithm_and_feedback_has_a_deterministic_distinct_wave() {
    let mut waves = [[0; 257]; 8];
    for (algorithm, wave) in waves.iter_mut().enumerate() {
        let mut channel = tone(algorithm as u8);
        for (index, offset) in [0, 4, 8, 12].into_iter().enumerate() {
            channel.write_operator(0x30 + offset, index as u8 + 1);
            channel.write_operator(0x40 + offset, index as u8 * 8);
        }
        channel.key(15);
        for sample in wave {
            *sample = channel.clock(None);
        }
    }
    for (index, wave) in waves.iter().enumerate() {
        assert!(wave.iter().any(|&sample| sample != 0));
        for other in &waves[index + 1..] {
            assert_ne!(wave, other);
        }
    }
    let mut plain = tone(7);
    plain.key(1);
    let mut feedback = plain;
    feedback.write_channel(0xb0, 0x3f);
    assert!((0..512).any(|_| plain.clock(None) != feedback.clock(None)));
}

#[test]
fn all_eight_algorithms_select_the_documented_carriers() {
    for (algorithm, carriers) in [8_u8, 8, 8, 8, 10, 14, 14, 15].into_iter().enumerate() {
        for slot in 0..4 {
            let mut channel = tone(algorithm as u8);
            channel.key(1 << slot);
            let audible = (0..256).any(|_| channel.clock(None) != 0);
            assert_eq!(audible, carriers & (1 << slot) != 0);
        }
    }
}

#[test]
fn channel_three_special_frequencies_follow_logical_operator_order() {
    for slot in 0..3 {
        let mut normal = tone(7);
        normal.key(1 << slot);
        let mut special = normal;
        let mut frequencies = [(1024, 3); 3];
        frequencies[slot] = (0, 0);
        let mut heard_normal = false;
        for _ in 0..512 {
            heard_normal |= normal.clock(None) != 0;
            assert_eq!(special.clock(Some(frequencies)), 0);
        }
        assert!(heard_normal);
    }
}

#[test]
fn zero_attack_stays_silent_and_sustain_rate_can_decay_without_first_decay() {
    let mut clock = EnvelopeClock::default();
    let mut silent = Operator::default();
    silent.write(5, 0xc0);
    silent.key(true, (0x7ff, 7));
    for _ in 0..4096 {
        if let Some(counter) = clock.clock() {
            silent.clock_envelope((0x7ff, 7), counter);
        }
        assert_eq!(silent.clock((0x7ff, 7), 0), 0);
    }
    let mut channel = tone(7);
    channel.write_operator(0x7c, 31);
    channel.key(8);
    assert!((0..128).any(|_| channel.clock_with_lfo(None, 0, clock.clock()) != 0));
    for _ in 0..1024 {
        channel.clock_with_lfo(None, 0, clock.clock());
    }
    assert!(!channel.active());
}

#[test]
fn key_scaling_accelerates_attack_and_first_decay_reaches_sustain() {
    let mut clock = EnvelopeClock::default();
    let mut low = Operator::default();
    low.write(5, 0xc8);
    low.key(true, (0, 0));
    let mut high = low;
    let mut low_level = 0;
    let mut high_level = 0;
    for _ in 0..4096 {
        if let Some(counter) = clock.clock() {
            low.clock_envelope((0, 0), counter);
            high.clock_envelope((0, 7), counter);
        }
        low_level = low.clock((0, 0), 256);
        high_level = high.clock((0, 7), 256);
    }
    assert!(high_level > low_level);
    let mut held = Operator::default();
    held.write(5, 31);
    held.write(6, 31);
    held.write(8, 0x40);
    held.key(true, (0, 0));
    let initial = held.clock((0, 0), 256);
    for _ in 0..1024 {
        if let Some(counter) = clock.clock() {
            held.clock_envelope((0, 0), counter);
        }
        held.clock((0, 0), 256);
    }
    let sustained = held.clock((0, 0), 256);
    assert!(sustained > 0 && sustained < initial);
    for _ in 0..1024 {
        if let Some(counter) = clock.clock() {
            held.clock_envelope((0, 0), counter);
        }
        assert_eq!(held.clock((0, 0), 256), sustained);
    }
}

#[test]
fn total_level_attenuates_output_and_detune_has_signed_effect() {
    let mut loud = tone(7);
    loud.key(8);
    let mut quiet = loud;
    quiet.write_operator(0x4c, 8);
    for _ in 0..64 {
        loud.clock(None);
        quiet.clock(None);
    }
    let loud_peak = loud.clock(None);
    let quiet_peak = quiet.clock(None);
    assert_eq!(loud_peak, 8160);
    assert_eq!(quiet_peak, 4064);
    let mut positive = loud;
    positive.write_operator(0x3c, 0x31);
    let mut negative = loud;
    negative.write_operator(0x3c, 0x71);
    let positive_wave: Vec<_> = (0..512).map(|_| positive.clock(None)).collect();
    let negative_wave: Vec<_> = (0..512).map(|_| negative.clock(None)).collect();
    assert_ne!(positive_wave, negative_wave);
}

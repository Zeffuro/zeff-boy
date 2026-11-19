use super::*;
use crate::hardware::audio::opn2::Opn2;
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[test]
fn slow_envelope_steps_repeat_after_the_timer_carry_is_consumed() {
    let mut clock = EnvelopeClock::default();
    let mut steps = Vec::new();
    for frame in 1..=30_720 {
        if clock
            .clock()
            .is_some_and(|counter| increment(2, counter) != 0)
        {
            steps.push(frame);
        }
    }
    assert_eq!(steps, [6144, 18_429, 30_714]);
}

#[test]
fn high_rate_double_steps_fall_between_low_rate_steps() {
    for counter in 1..=16 {
        assert_eq!(increment(44, counter), counter & 1);
        assert_eq!(increment(49, counter), 1 + u16::from(counter % 4 == 0));
        assert_eq!(increment(50, counter), 1 + u16::from(counter % 2 == 0));
        assert_eq!(increment(51, counter), 1 + u16::from(counter % 4 != 3));
    }
}

#[test]
fn discrete_rates_follow_measured_patterns_and_update_intervals() {
    for (rate, shift, pattern) in [
        (0, 11, [0; 8]),
        (1, 11, [0; 8]),
        (2, 11, [0, 1, 0, 1, 0, 1, 0, 1]),
        (3, 11, [0, 1, 0, 1, 0, 1, 0, 1]),
        (5, 10, [0, 1, 0, 1, 0, 1, 0, 1]),
        (7, 10, [0, 1, 1, 1, 0, 1, 1, 1]),
        (9, 9, [0, 1, 0, 1, 1, 1, 0, 1]),
        (45, 0, [0, 1, 0, 1, 1, 1, 0, 1]),
        (49, 0, [2, 1, 1, 1, 2, 1, 1, 1]),
        (53, 0, [4, 2, 2, 2, 4, 2, 2, 2]),
        (57, 0, [8, 4, 4, 4, 8, 4, 4, 4]),
        (63, 0, [8; 8]),
    ] {
        for (index, expected) in pattern.into_iter().enumerate() {
            let counter = (index as u16) << shift;
            assert_eq!(increment(rate, counter), expected, "rate={rate}");
            if shift != 0 {
                assert_eq!(increment(rate, counter + 1), 0);
            }
        }
    }
    let totals = [0, 0, 4, 4, 4, 4, 6, 6];
    for rate in 0..64 {
        let shift = 11_u8.saturating_sub(rate >> 2);
        let sum: u16 = (0..8).map(|index| increment(rate, index << shift)).sum();
        let expected = match rate {
            0..=7 => totals[usize::from(rate)],
            8..=47 => [4, 5, 6, 7][usize::from(rate & 3)],
            48..=59 => [8, 10, 12, 14][usize::from(rate & 3)] << ((rate - 48) >> 2),
            _ => 64,
        };
        assert_eq!(sum, expected, "rate={rate}");
    }
}

#[test]
fn clock_divider_wrap_idle_and_bulk_advance_preserve_the_shared_calendar() {
    let mut clock = EnvelopeClock::default();
    assert_eq!(clock.clock(), None);
    assert_eq!(clock.clock(), None);
    assert_eq!(clock.clock(), Some(1));
    for phase in 0..3 {
        for counter in [0, 1, 1023, 2047, ENVELOPE_COUNTER_MAX] {
            for clocks in [0, 1, 2, 3, 7, 49_153] {
                let mut bulk = EnvelopeClock { counter, phase };
                let mut single = bulk;
                bulk.advance(clocks);
                for _ in 0..clocks {
                    single.clock();
                }
                assert_eq!(bulk, single);
            }
        }
    }
    let mut whole = EnvelopeClock::default();
    let mut split = whole;
    whole.advance(u64::MAX);
    split.advance(u64::MAX / 2);
    split.advance(u64::MAX / 2 + 1);
    assert_eq!(whole, split);
    let mut whole = Opn2::default();
    let mut split = whole;
    let mut active = whole;
    active.write_port(0, 0x28);
    active.write_port(1, 0x10);
    whole.advance(144 * 49_160 + 71);
    for clocks in [1, 144 * 20_000, 70, 144 * 29_160] {
        split.advance(clocks);
        active.advance(clocks);
    }
    assert_eq!(whole, split);
    assert_eq!(whole.envelope_clock, active.envelope_clock);
}

#[test]
fn active_envelopes_continue_across_wrap_rate_writes_and_state() {
    let mut whole = Opn2::default();
    whole.advance(144 * (12_285 - 2) + 29);
    for (address, value) in [
        (0x30, 1),
        (0x50, 24),
        (0x60, 23),
        (0x70, 8),
        (0x80, 0x3f),
        (0xb0, 7),
        (0xa4, 0x1c),
        (0xa0, 0xbf),
        (0x28, 0x10),
    ] {
        whole.write_port(0, address);
        whole.write_port(1, value);
    }
    let mut split = whole;
    let mut writer = StateWriter::new();
    whole.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut restored = Opn2::read_state(&mut StateReader::new(&bytes)).unwrap();
    let mut audible = false;
    for tick in 0..1024 {
        if tick == 17 || tick == 300 {
            for chip in [&mut whole, &mut split, &mut restored] {
                chip.write_port(0, 0x50);
                chip.write_port(1, if tick == 17 { 26 } else { 24 });
            }
        }
        whole.advance(144);
        for clocks in [1, 28, 115] {
            split.advance(clocks);
        }
        restored.advance(144);
        let sample = whole.sample_with_mutes([false; 6]);
        audible |= sample != [0, 0];
        assert_eq!(sample, split.sample_with_mutes([false; 6]));
        assert_eq!(whole, restored);
    }
    assert!(audible);
    assert_eq!(whole, split);
}

use zeff_md_core::Psg;

fn write_period(psg: &mut Psg, channel: u8, period: u16) {
    psg.write_data(0x80 | channel << 5 | (period as u8 & 15));
    psg.write_data((period >> 4) as u8);
}

pub fn register_decoder() {
    for register in 0..8 {
        for value in 0..=255u8 {
            let mut psg = Psg::default();
            psg.write_data(0x80 | register << 4 | 5);
            let before = psg.debug_snapshot();
            psg.write_data(value);
            let actual = psg.debug_snapshot();
            let selected = if value & 0x80 != 0 {
                value >> 4 & 7
            } else {
                register
            };
            assert_eq!(actual.latched_register, selected);
            if selected & 1 != 0 {
                assert_eq!(actual.volumes[usize::from(selected >> 1)], value & 15);
                assert_eq!(actual.periods, before.periods);
            } else if selected == 6 {
                assert_eq!(actual.noise_control, value & 7);
                assert_eq!(actual.noise_lfsr, 0x8000);
                assert_eq!(actual.periods, before.periods);
            } else {
                let channel = usize::from(selected >> 1);
                let expected = if value & 0x80 != 0 {
                    before.periods[channel] & 0x3f0 | u16::from(value & 15)
                } else {
                    before.periods[channel] & 15 | u16::from(value & 63) << 4
                };
                assert_eq!(actual.periods[channel], expected);
                assert!(actual.periods.iter().all(|&period| period <= 0x3ff));
            }
            assert_eq!(actual.counters, before.counters);
            assert_eq!(actual.outputs, before.outputs);
            assert_eq!(actual.noise_counter, before.noise_counter);
            assert_eq!(actual.noise_clock, before.noise_clock);
        }
    }
}

pub fn reloads_preserve_countdowns() {
    for channel in 0..3 {
        let mut psg = Psg::default();
        write_period(&mut psg, channel, 1000);
        psg.advance(2);
        let before = psg.debug_snapshot();
        assert_eq!(before.counters[usize::from(channel)], 999);
        write_period(&mut psg, channel, 3);
        assert_eq!(psg.debug_snapshot().counters, before.counters);
        psg.advance(998);
        assert_eq!(psg.debug_snapshot().counters[usize::from(channel)], 1);
        psg.advance(1);
        assert_eq!(psg.debug_snapshot().counters[usize::from(channel)], 3);
        assert!(psg.debug_snapshot().outputs[usize::from(channel)]);
    }
}

pub fn noise_sequences_and_rates() {
    for rate in 0..3 {
        for white in [false, true] {
            let mut psg = Psg::default();
            psg.write_data(0xe0 | rate | u8::from(white) << 2);
            let half = 16u64 << rate;
            psg.advance(16);
            assert_eq!(psg.debug_snapshot().noise_lfsr, 0x8000);
            let mut expected = 0x8000u16;
            for _ in 0..128 {
                psg.advance(half);
                let bit = if white {
                    expected ^ (expected >> 3)
                } else {
                    expected
                } & 1;
                expected = expected >> 1 | bit << 15;
                assert_eq!(psg.debug_snapshot().noise_lfsr, expected);
                psg.advance(half);
                assert_eq!(psg.debug_snapshot().noise_lfsr, expected);
            }
            let before = psg.debug_snapshot();
            psg.write_data(0xf0);
            assert_eq!(psg.debug_snapshot().noise_lfsr, before.noise_lfsr);
            psg.write_data(0xe4 | rate);
            assert_eq!(psg.debug_snapshot().noise_lfsr, 0x8000);
            assert_eq!(psg.debug_snapshot().noise_counter, before.noise_counter);
            assert_eq!(psg.debug_snapshot().noise_clock, before.noise_clock);
            psg.advance(1);
            let before = psg.debug_snapshot();
            psg.write_data(4 | rate);
            assert_eq!(psg.debug_snapshot().noise_lfsr, 0x8000);
            assert_eq!(psg.debug_snapshot().noise_counter, before.noise_counter);
        }
    }
    let mut psg = Psg::default();
    psg.write_data(0xe0);
    psg.advance(32 * 16);
    assert_eq!(psg.debug_snapshot().noise_lfsr, 0x8000);
    psg.write_data(0xe4);
    psg.advance(32 * 16);
    assert_eq!(psg.debug_snapshot().noise_lfsr, 0x9000);
}

pub fn digital_clock_and_pcm_policy() {
    for period in [0, 1, 7, 1023] {
        let mut psg = Psg::default();
        write_period(&mut psg, 2, period);
        psg.write_data(0xe3);
        psg.write_data(0xd0);
        psg.advance(1);
        assert_eq!(psg.debug_snapshot().noise_lfsr, 0x8000);
        psg.advance(u64::from(period.max(1)));
        assert_eq!(psg.debug_snapshot().noise_lfsr, 0x4000);
        if period <= 1 {
            assert_eq!(psg.sample(), 8191);
        }
        psg.advance(u64::from(period.max(1)));
        assert_eq!(psg.debug_snapshot().noise_lfsr, 0x4000);
        psg.advance(u64::from(period.max(1)));
        assert_eq!(psg.debug_snapshot().noise_lfsr, 0x2000);
    }
}

pub fn jump_and_partitioned_clocks() {
    for control in 0..8 {
        let mut whole = Psg::default();
        for (channel, period) in [13, 273, 65].into_iter().enumerate() {
            write_period(&mut whole, channel as u8, period);
        }
        whole.write_data(0xe0 | control);
        let mut single = whole;
        whole.advance(250_000);
        for _ in 0..250_000 {
            single.advance(1);
        }
        assert_eq!(whole, single);
        let mut parts = single;
        whole.advance(u64::MAX);
        parts.advance(u64::MAX / 2);
        parts.advance(u64::MAX - u64::MAX / 2);
        assert_eq!(whole, parts);
    }
    let mut whole = Psg::default();
    let mut parts = whole;
    whole.advance(u64::MAX);
    parts.advance(1);
    parts.advance(u64::MAX - 1);
    assert_eq!(whole, parts);
}

pub fn attenuation_and_mix() {
    let expected = [
        8191, 6506, 5168, 4105, 3261, 2590, 2057, 1634, 1298, 1031, 819, 651, 517, 411, 326, 0,
    ];
    for (volume, &level) in expected.iter().enumerate() {
        let mut psg = Psg::default();
        assert_eq!(psg.sample(), 0);
        psg.write_data(0x90 | volume as u8);
        assert_eq!(psg.sample(), level);
        write_period(&mut psg, 0, 4);
        psg.advance(1);
        assert_eq!(psg.sample(), -level);
    }
    let mut psg = Psg::default();
    for value in [0x90, 0xb0, 0xd0, 0xf0] {
        psg.write_data(value);
    }
    assert_eq!(psg.sample(), 8191 * 2);
    psg.advance(32 * 15);
    assert_eq!(psg.sample(), 8191 * 4);
}

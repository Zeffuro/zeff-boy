use super::*;

fn register(audio: &mut Audio, at: u64, bank: u8, address: u8, value: u8) {
    audio.write_opn2(at, bank * 2, address);
    audio.write_opn2(at, bank * 2 + 1, value);
}

fn tick_to(audio: &mut Audio, at: u64) {
    while audio.now < at {
        audio.advance_chip(audio.now + 1);
        audio.sample_phase += u64::from(audio.sample_rate);
        audio.refresh_output();
        if audio.sample_phase >= audio.master_hz {
            let value = audio.output.sample();
            audio.push(value);
            audio.sample_phase -= audio.master_hz;
        }
    }
}

#[test]
fn fm_latches_reach_the_filter_at_their_master_tick_between_psg_events() {
    for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
        for rate in [44_100, 48_000, 192_000] {
            let mut actual = Audio::new(region);
            actual.set_sample_rate(rate);
            for channel in 0..6 {
                let bank = channel / 3;
                let index = channel % 3;
                for slot in [0, 4, 8, 12] {
                    for (group, value) in [
                        (0x30, 1),
                        (0x40, if slot == 12 { 0 } else { 127 }),
                        (0x50, 31),
                    ] {
                        register(&mut actual, 0, bank, group + slot + index, value);
                    }
                }
                for (group, value) in [(0xb0, 7), (0xb4, 0xc0), (0xa4, 0x1c), (0xa0, 0)] {
                    register(&mut actual, 0, bank, group + index, value);
                }
                register(&mut actual, 0, 0, 0x28, 0x80 | index | bank << 2);
            }
            for value in [0x85, 0x06, 0x90] {
                actual.write_psg(0, value);
            }
            let mut expected = actual.clone();
            for at in [1, 41, 42, 43, 211, 1007, 1009, 20_003, 55_999, 80_003] {
                actual.advance_to(at);
                tick_to(&mut expected, at);
                assert_eq!(actual, expected, "{region:?} {rate} {at}");
                for audio in [&mut actual, &mut expected] {
                    let channel = (at % 6) as u8;
                    register(audio, at, channel / 3, 0xb4 + channel % 3, 0x80);
                    register(audio, at, 0, 0x2a, at as u8);
                    register(audio, at, 0, 0x2b, if at % 2 == 0 { 0x80 } else { 0 });
                    audio.write_psg(at, 0x90 | (at % 8) as u8);
                }
            }
            let mut pcm = Vec::new();
            actual.drain_into(&mut pcm);
            assert!(pcm.iter().any(|value| *value != 0.0));
        }
    }
}

#[test]
fn long_audio_gaps_preserve_retained_pcm_and_future_filter_output() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [44_100, 48_000, 192_000] {
            let mut seed = Audio::new(region);
            seed.set_sample_rate(rate);
            for value in [0x85, 0x06, 0x90] {
                seed.write_psg(0, value);
            }
            register(&mut seed, 0, 0, 0x2a, 0xc1);
            register(&mut seed, 0, 0, 0x2b, 0x80);
            seed.advance_to(37_001);
            seed.drain_into(&mut Vec::new());
            let retained = (SAMPLE_CAPACITY + output::HISTORY) as u64;
            for count in [retained - 1, retained, retained + 1, retained * 2] {
                let mut actual = seed.clone();
                let mut expected = seed.clone();
                let at = seed.now
                    + (count * seed.master_hz - seed.sample_phase).div_ceil(u64::from(rate));
                actual.advance_to(at);
                while expected.now < at {
                    expected.advance_to((expected.now + 137_003).min(at));
                }
                assert_eq!(actual.now, expected.now);
                assert_eq!(actual.sample_phase, expected.sample_phase);
                assert_eq!(actual.chip_phase, expected.chip_phase);
                assert_eq!(actual.opn2_phase, expected.opn2_phase);
                assert_eq!(actual.psg, expected.psg);
                assert_eq!(actual.opn2, expected.opn2);
                for end in [at, at + 80_003] {
                    actual.advance_to(end);
                    expected.advance_to(end);
                    let mut pcm = Vec::new();
                    let mut control = Vec::new();
                    actual.drain_into(&mut pcm);
                    expected.drain_into(&mut control);
                    assert!(!pcm.is_empty() && pcm.iter().any(|value| *value != 0.0));
                    assert_eq!(pcm, control, "{region:?} {rate} {count} {end}");
                }
            }
        }
    }
}

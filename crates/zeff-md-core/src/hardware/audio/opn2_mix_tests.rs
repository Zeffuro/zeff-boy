use super::*;

fn register(audio: &mut Audio, at: u64, bank: u8, address: u8, value: u8) {
    audio.write_opn2(at, bank * 2, address);
    audio.write_opn2(at, bank * 2 + 1, value);
}

fn psg_audio(region: Region) -> Audio {
    let mut audio = Audio::new(region);
    for value in [0x85, 0x06, 0x90] {
        audio.write_psg(0, value);
    }
    audio
}

fn mixed_audio(region: Region) -> Audio {
    let mut audio = psg_audio(region);
    for slot in [0, 4, 8, 12] {
        register(&mut audio, 0, 0, 0x30 + slot, 1);
        register(
            &mut audio,
            0,
            0,
            0x40 + slot,
            if slot == 12 { 0 } else { 127 },
        );
        register(&mut audio, 0, 0, 0x50 + slot, 31);
        register(&mut audio, 0, 0, 0x80 + slot, 15);
    }
    for (address, value) in [
        (0xb0, 7),
        (0xb4, 0x80),
        (0xa4, 0x1c),
        (0xa0, 0),
        (0x28, 0x80),
        (0x26, 0xff),
        (0x27, 0x0a),
        (0x2a, 255),
        (0x2b, 0x80),
    ] {
        register(&mut audio, 0, 0, address, value);
    }
    register(&mut audio, 0, 1, 0xb6, 0x40);
    audio
}

fn drain(audio: &mut Audio) -> Vec<f32> {
    let mut samples = Vec::new();
    audio.drain_into(&mut samples);
    samples
}

#[test]
fn pal_and_ntsc_dac_mix_preserves_the_psg_wave_on_each_stereo_side() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut baseline = psg_audio(region);
        let mut mixed = baseline.clone();
        register(&mut mixed, 0, 1, 0xb6, 0x80);
        register(&mut mixed, 0, 0, 0x2a, 255);
        register(&mut mixed, 0, 0, 0x2b, 0x80);
        baseline.advance_to(500_000);
        mixed.advance_to(500_000);
        assert_eq!(baseline.psg, mixed.psg);
        let pure = drain(&mut baseline);
        let combined = drain(&mut mixed);
        assert_eq!(combined.len(), pure.len());
        assert!(pure.iter().any(|sample| *sample > 0.0));
        assert!(pure.iter().any(|sample| *sample < 0.0));
        for (source, output) in pure
            .as_chunks::<2>()
            .0
            .iter()
            .zip(combined.as_chunks::<2>().0.iter())
            .skip(output::HISTORY)
        {
            assert_eq!(source[0], source[1]);
            assert_eq!(output[0], source[0] + 8128.0 / 32768.0);
            assert_eq!(output[1], source[1]);
        }
    }
}

#[test]
fn six_fm_carriers_clip_after_stereo_mixing() {
    let mut audio = Audio::default();
    for channel in 0..6 {
        let at = u64::from(channel) * 1008 * 4;
        let bank = channel / 3;
        let index = channel % 3;
        for slot in [0, 4, 8, 12] {
            for (group, value) in [
                (0x30, 1),
                (0x40, if slot == 12 { 0 } else { 127 }),
                (0x50, 31),
            ] {
                register(&mut audio, at, bank, group + slot + index, value);
            }
        }
        for (address, value) in [(0xb0, 7), (0xb4, 0xc0), (0xa4, 0x22), (0xa0, 0)] {
            register(&mut audio, at, bank, address + index, value);
        }
        register(&mut audio, at, 0, 0x28, 0x80 | index | bank << 2);
    }
    audio.advance_to(500_000);
    let samples = drain(&mut audio);
    assert!(samples.contains(&(32767.0 / 32768.0)));
    assert!(samples.contains(&-1.0));
    assert!(
        samples
            .as_chunks::<2>()
            .0
            .iter()
            .all(|pair| pair[0] == pair[1])
    );
}

#[test]
fn stereo_fm_and_dac_use_selected_region_cadence_and_whole_split_state() {
    for (region, master) in [(Region::NtscUs, 53_693_175_u64), (Region::Pal, 53_203_424)] {
        for rate in [1, 44_100, 48_000, 192_000] {
            let mut whole = mixed_audio(region);
            whole.set_sample_rate(rate);
            let mut parts = whole.clone();
            let end = 532_037;
            whole.advance_to(end);
            let mut at = 0;
            while at < end {
                at = (at + 137).min(end);
                parts.advance_to(at);
            }
            assert_eq!(whole.len, (end * u64::from(rate) / master) as usize);
            assert_eq!(whole, parts);
            let output = drain(&mut whole);
            if rate > 1 {
                assert!(
                    output
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .any(|pair| pair[0] != pair[1])
                );
            }
        }
    }
}

#[test]
fn checkpoint_restores_fm_dac_timers_stereo_storage_and_pending_clock_phases() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut audio = mixed_audio(region);
        audio.set_sample_rate(44_100);
        audio.set_fm_channel_mutes([false, true, false, true, false, false]);
        audio.advance_to(123_457);
        let saved = audio.clone();
        let mut continued = audio.clone();
        let pointer = audio.samples.as_ptr();
        let checkpoint = audio.checkpoint();
        register(&mut audio, 123_457, 0, 0x28, 0);
        register(&mut audio, 123_457, 0, 0x2a, 0);
        register(&mut audio, 123_457, 0, 0x27, 0x30);
        audio.set_fm_channel_mutes([true; 6]);
        audio.set_sample_rate(192_000);
        audio.advance_to(250_003);
        audio.restore(checkpoint);
        assert_eq!(audio, saved);
        assert_eq!(audio.samples.as_ptr(), pointer);
        audio.advance_to(532_037);
        continued.advance_to(532_037);
        assert_eq!(audio, continued);
        assert_eq!(drain(&mut audio), drain(&mut continued));
    }
}

#[test]
fn disabled_generation_and_all_mutes_keep_fm_psg_and_timers_running() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut ordinary = mixed_audio(region);
        let mut disabled = ordinary.clone();
        let mut muted = ordinary.clone();
        disabled.set_sample_generation_enabled(false);
        muted.set_channel_mutes([true; PSG_CHANNELS]);
        muted.set_fm_channel_mutes([true; 6]);
        for at in [1009, 123_457, 500_003] {
            ordinary.advance_to(at);
            disabled.advance_to(at);
            muted.advance_to(at);
            assert_eq!(ordinary.opn2, disabled.opn2);
            assert_eq!(ordinary.opn2, muted.opn2);
            assert_eq!(ordinary.psg, disabled.psg);
            assert_eq!(ordinary.psg, muted.psg);
            assert_eq!(ordinary.opn2_phase, disabled.opn2_phase);
            assert_eq!(ordinary.sample_phase, disabled.sample_phase);
        }
        assert_eq!(disabled.len, 0);
        assert!(drain(&mut muted).into_iter().all(|sample| sample == 0.0));
        drain(&mut ordinary);
        disabled.set_sample_generation_enabled(true);
        muted.set_channel_mutes([false; PSG_CHANNELS]);
        muted.set_fm_channel_mutes([false; 6]);
        for audio in [&mut ordinary, &mut disabled, &mut muted] {
            audio.advance_to(700_013);
        }
        let expected = drain(&mut ordinary);
        assert_eq!(
            expected[output::HISTORY * 2..],
            drain(&mut disabled)[output::HISTORY * 2..]
        );
        assert_eq!(
            expected[output::HISTORY * 2..],
            drain(&mut muted)[output::HISTORY * 2..]
        );
    }
}

#[test]
fn disabled_generation_checkpoint_restores_for_exact_reenabled_continuation() {
    let mut audio = mixed_audio(Region::Pal);
    audio.set_sample_generation_enabled(false);
    audio.advance_to(123_457);
    let mut continued = audio.clone();
    let saved = audio.clone();
    let checkpoint = audio.checkpoint();
    audio.set_sample_generation_enabled(true);
    audio.reset_opn2(audio.now);
    audio.advance_to(500_003);
    audio.restore(checkpoint);
    assert_eq!(audio, saved);
    for instance in [&mut audio, &mut continued] {
        instance.advance_to(250_001);
        assert_eq!(instance.len, 0);
        instance.set_sample_generation_enabled(true);
        instance.advance_to(700_013);
    }
    assert_eq!(audio, continued);
}

#[test]
fn opn2_status_prediction_uses_master_divider_and_retained_fraction_without_mutation() {
    let mut audio = Audio::default();
    register(&mut audio, 5, 0, 0x2a, 255);
    let saved = audio.clone();
    assert_eq!(audio.opn2_status_at(1343), 0x80);
    assert_eq!(audio.opn2_status_at(1344), 0);
    assert_eq!(audio, saved);
    audio.advance_to(1000);
    let saved = audio.clone();
    assert_eq!(audio.opn2_status_at(1343), 0x80);
    assert_eq!(audio.opn2_status_at(1344), 0);
    assert_eq!(audio, saved);
    audio.advance_to(1344);
    assert_eq!(audio.opn2.status(0), 0);
}

#[test]
fn exact_dac_write_endpoint_publishes_old_sample_then_new_stereo_level() {
    let mut audio = Audio::default();
    register(&mut audio, 0, 1, 0xb6, 0x40);
    register(&mut audio, 0, 0, 0x2b, 0x80);
    let at = audio.master_hz.div_ceil(u64::from(audio.sample_rate));
    register(&mut audio, at, 0, 0x2a, 255);
    assert_eq!(drain(&mut audio), [0.0; 2]);
    audio.advance_to(at * (output::HISTORY as u64 + 4));
    let pcm = drain(&mut audio);
    assert!(pcm.as_chunks::<2>().0.iter().all(|pair| pair[0] == 0.0));
    assert!(
        pcm[output::HISTORY * 2..]
            .as_chunks::<2>()
            .0
            .iter()
            .all(|pair| pair[1] == 8128.0 / 32768.0)
    );
}

#[test]
fn opn2_reset_keeps_psg_time_output_and_selected_region() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut audio = mixed_audio(region);
        let mut baseline = psg_audio(region);
        audio.advance_to(123_457);
        baseline.advance_to(123_457);
        drain(&mut audio);
        drain(&mut baseline);
        audio.reset_opn2(audio.now);
        assert_eq!(audio.opn2, Opn2::default());
        assert_eq!(audio.opn2_phase, 0);
        assert_eq!(audio.psg, baseline.psg);
        assert_eq!(audio.now, baseline.now);
        audio.advance_to(500_003);
        baseline.advance_to(500_003);
        assert_eq!(
            drain(&mut audio)[output::HISTORY * 2..],
            drain(&mut baseline)[output::HISTORY * 2..]
        );
    }
}

#[test]
fn mixed_ring_overflow_keeps_identical_latest_stereo_samples() {
    let mut whole = mixed_audio(Region::Pal);
    let mut parts = whole.clone();
    let end = 7_000_013;
    whole.advance_to(end);
    for index in 1..=100 {
        parts.advance_to(end * index / 100);
    }
    assert_eq!(whole.opn2, parts.opn2);
    assert_eq!(whole.psg, parts.psg);
    assert_eq!(whole.len, SAMPLE_CAPACITY);
    assert_eq!(drain(&mut whole), drain(&mut parts));
}

use super::*;
use crate::constants::NTSC_MASTER_HZ;

fn tone() -> Audio {
    tone_with_region(Region::NtscUs)
}

fn tone_with_region(region: Region) -> Audio {
    let mut audio = Audio::new(region);
    for value in [0x85, 0x06, 0x90] {
        audio.write_psg(0, value);
    }
    audio
}

#[test]
fn pal_exact_cadence_rates_partition_and_chip_clock() {
    for rate in [1, 44_100, 48_000, 192_000] {
        let mut whole = tone_with_region(Region::Pal);
        whole.set_sample_rate(rate);
        let mut parts = whole.clone();
        let end = 532_035;
        whole.advance_to(end);
        let mut at = 0;
        while at < end {
            at = (at + 137).min(end);
            parts.advance_to(at);
        }
        let expected = (u128::from(end) * u128::from(rate) / 53_203_424) as usize;
        assert_eq!(whole.len, expected);
        assert_eq!(whole.sample_phase, (end * u64::from(rate)) % 53_203_424);
        assert_eq!(whole, parts);
        let mut ntsc = tone();
        ntsc.advance_to(end);
        assert_eq!(whole.psg, ntsc.psg);
        assert_eq!(whole.chip_phase, ntsc.chip_phase);
    }
    assert_eq!(Audio::default(), Audio::new(Region::NtscUs));
    assert_eq!(Audio::default(), Audio::new(Region::NtscJapan));
}

#[test]
fn pal_one_second_drains_exact_configured_samples() {
    for rate in [1, 44_100, 48_000, 192_000] {
        let mut audio = tone_with_region(Region::Pal);
        audio.set_sample_rate(rate);
        let mut output = Vec::new();
        for index in 1..=1000 {
            audio.advance_to(53_203_424 * index / 1000);
            audio.drain_into(&mut output);
        }
        assert_eq!(output.len(), rate as usize * OUTPUT_CHANNELS);
        assert_eq!(audio.sample_phase, 0);
    }
}

#[test]
fn pal_checkpoint_reset_mutes_and_disabled_output_keep_clock() {
    let mut audio = tone_with_region(Region::Pal);
    audio.set_sample_rate(44_100);
    audio.advance_to(123_456);
    let saved = audio.clone();
    let pointer = audio.samples.as_ptr();
    let checkpoint = audio.checkpoint();
    audio.write_psg(audio.now, 0x9f);
    audio.advance_to(250_000);
    audio.restore(checkpoint);
    assert_eq!(audio, saved);
    assert_eq!(audio.samples.as_ptr(), pointer);
    let mut disabled = audio.clone();
    let mut muted = audio.clone();
    disabled.set_sample_generation_enabled(false);
    muted.set_channel_mutes([true; PSG_CHANNELS]);
    audio.drain_into(&mut Vec::new());
    muted.drain_into(&mut Vec::new());
    for at in [150_000, 200_000, 532_035] {
        audio.advance_to(at);
        disabled.advance_to(at);
        muted.advance_to(at);
        assert_eq!(audio.psg, disabled.psg);
        assert_eq!(audio.psg, muted.psg);
        assert_eq!(audio.sample_phase, disabled.sample_phase);
        assert_eq!(audio.sample_phase, muted.sample_phase);
    }
    assert_eq!(disabled.len, 0);
    assert_eq!(muted.len, audio.len);
    let mut output = Vec::new();
    muted.drain_into(&mut output);
    assert!(output.iter().all(|&sample| sample == 0.0));
    for instance in [&mut audio, &mut disabled, &mut muted] {
        instance.reset();
        assert_eq!(instance.master_hz, 53_203_424);
        assert_eq!(instance.sample_rate(), 44_100);
        instance.advance_to(532_035);
        assert_eq!(instance.sample_phase, (532_035 * 44_100) % 53_203_424);
    }
    assert!(!disabled.generation_enabled);
    assert_eq!(muted.mutes, [true; PSG_CHANNELS]);
    assert_eq!(audio.samples.as_ptr(), pointer);
}

#[test]
fn rational_cadence_and_partitioned_samples() {
    for rate in [1, 44_100, 48_000, 192_000] {
        let mut whole = tone();
        whole.set_sample_rate(rate);
        let mut parts = whole.clone();
        let end = NTSC_MASTER_HZ / 100;
        whole.advance_to(end);
        let mut at = 0;
        while at < end {
            at = (at + 137).min(end);
            parts.advance_to(at);
        }
        let expected = (u128::from(end) * u128::from(rate) / u128::from(NTSC_MASTER_HZ)) as usize;
        assert_eq!(whole.len, expected);
        assert_eq!(whole, parts);
        let mut output = Vec::new();
        whole.drain_into(&mut output);
        assert_eq!(output.len(), expected * 2);
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .all(|pair| pair[0] == pair[1])
        );
        if expected > 2 {
            assert!(output.iter().any(|&sample| sample > 0.0));
            assert!(output.iter().any(|&sample| sample < 0.0));
        }
        let saved = output.clone();
        whole.drain_into(&mut output);
        assert_eq!(output, saved);
    }
}

#[test]
fn disabled_and_muted_output_keeps_digital_clock() {
    let mut ordinary = tone();
    ordinary.write_psg(0, 0xe7);
    ordinary.write_psg(0, 0xf0);
    let mut disabled = ordinary.clone();
    let mut muted = ordinary.clone();
    disabled.set_sample_generation_enabled(false);
    muted.set_channel_mutes([true; PSG_CHANNELS]);
    for at in [10_000, 100_000, 200_000] {
        ordinary.advance_to(at);
        disabled.advance_to(at);
        muted.advance_to(at);
        assert_eq!(ordinary.psg, disabled.psg);
        assert_eq!(ordinary.psg, muted.psg);
        assert_eq!(ordinary.sample_phase, disabled.sample_phase);
    }
    assert_eq!(disabled.len, 0);
    assert_eq!(muted.len, ordinary.len);
    let mut output = Vec::new();
    muted.drain_into(&mut output);
    assert!(output.iter().all(|&sample| sample == 0.0));
    ordinary.drain_into(&mut Vec::new());
    disabled.set_sample_generation_enabled(true);
    muted.set_channel_mutes([false; PSG_CHANNELS]);
    ordinary.advance_to(300_000);
    disabled.advance_to(300_000);
    muted.advance_to(300_000);
    let mut output = Vec::new();
    ordinary.drain_into(&mut output);
    let mut recovered = Vec::new();
    disabled.drain_into(&mut recovered);
    assert_eq!(
        output[output::HISTORY * 2..],
        recovered[output::HISTORY * 2..]
    );
    recovered.clear();
    muted.drain_into(&mut recovered);
    assert_eq!(
        output[output::HISTORY * 2..],
        recovered[output::HISTORY * 2..]
    );
    muted.set_channel_mutes([true, false, false, false]);
    assert_eq!(
        muted.psg.sample_with_mutes(muted.mutes),
        if muted.psg.debug_snapshot().noise_lfsr & 1 != 0 {
            8191
        } else {
            -8191
        }
    );
    disabled.set_sample_generation_enabled(false);
    disabled.reset();
    assert!(!disabled.generation_enabled);
    muted.reset();
    assert_eq!(muted.mutes, [true, false, false, false]);
}

#[test]
fn output_overflow_keeps_latest_samples() {
    let mut whole = tone();
    let mut parts = whole.clone();
    let end = NTSC_MASTER_HZ;
    whole.advance_to(end);
    for index in 1..=1000 {
        parts.advance_to(end * index / 1000);
    }
    assert_eq!(whole.psg, parts.psg);
    assert_eq!(whole.len, SAMPLE_CAPACITY);
    assert_eq!(whole.sample_phase, parts.sample_phase);
    let mut left = Vec::new();
    let mut right = Vec::new();
    whole.drain_into(&mut left);
    parts.drain_into(&mut right);
    assert_eq!(left, right);
}

#[test]
fn checkpoint_reset_and_rate_preserve_owned_storage() {
    let mut audio = tone();
    audio.advance_to(NTSC_MASTER_HZ / 100);
    let saved = audio.clone();
    let pointer = audio.samples.as_ptr();
    let checkpoint = audio.checkpoint();
    audio.write_psg(audio.now, 0xe7);
    audio.set_sample_rate(44_100);
    audio.advance_to(audio.now + 100_000);
    audio.restore(checkpoint);
    assert_eq!(audio, saved);
    assert_eq!(audio.samples.as_ptr(), pointer);
    audio.set_sample_rate(u32::MAX);
    assert_eq!(audio.sample_rate(), MAX_SAMPLE_RATE);
    assert_eq!(audio.len, 0);
    audio.set_sample_rate(0);
    assert_eq!(audio.sample_rate(), DEFAULT_SAMPLE_RATE);
    audio.set_sample_rate(44_100);
    audio.reset();
    assert_eq!(audio.sample_rate(), 44_100);
    assert_eq!(audio.psg, Psg::default());
    assert_eq!(audio.now, 0);
    assert!(
        audio
            .samples
            .iter()
            .all(|&sample| sample == [0; OUTPUT_CHANNELS])
    );
    assert_eq!(audio.samples.as_ptr(), pointer);
}

#[test]
fn exact_write_endpoint_samples_precede_new_volume() {
    let mut audio = Audio::default();
    for value in [0x81, 0, 0x90] {
        audio.write_psg(0, value);
    }
    audio.set_sample_rate(DEFAULT_SAMPLE_RATE);
    let at = NTSC_MASTER_HZ.div_ceil(u64::from(DEFAULT_SAMPLE_RATE));
    audio.write_psg(at, 0x9f);
    let mut queued = Vec::new();
    audio.drain_into(&mut queued);
    assert_eq!(queued, [1365.0 / 32768.0; 2]);
    audio.advance_to(at * (output::HISTORY as u64 + 4));
    let mut output = Vec::new();
    audio.drain_into(&mut output);
    assert!(output.iter().any(|sample| *sample != 0.0));
    assert!(
        output[output::HISTORY * 2..]
            .iter()
            .all(|sample| *sample == 0.0)
    );
}

#[test]
fn maximum_timeline_remains_bounded_and_partition_independent() {
    let mut whole = tone();
    let mut parts = whole.clone();
    whole.advance_to(u64::MAX);
    parts.advance_to(u64::MAX / 2);
    parts.advance_to(u64::MAX);
    assert_eq!(whole.psg, parts.psg);
    assert_eq!(whole.now, u64::MAX);
    assert_eq!(whole.len, SAMPLE_CAPACITY);
    let mut left = Vec::new();
    let mut right = Vec::new();
    whole.drain_into(&mut left);
    parts.drain_into(&mut right);
    assert_eq!(left, right);
}

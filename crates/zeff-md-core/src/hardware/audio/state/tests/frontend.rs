use super::*;

fn frontend(audio: &Audio) -> Vec<u8> {
    let mut writer = Writer::new();
    audio.write_frontend_state(&mut writer).unwrap();
    writer.into_bytes()
}

fn restore(source: &Audio, target: &mut Audio) {
    target
        .read_state(&mut Reader::new(&bytes(source)), Region::NtscUs)
        .unwrap();
    target
        .read_frontend_state(&mut Reader::new(&frontend(source)))
        .unwrap();
}

fn pcm(audio: &mut Audio) -> Vec<f32> {
    let mut samples = Vec::new();
    audio.drain_into(&mut samples);
    samples
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn uninterrupted_filter_and_queue_survive_all_rates_regions_and_wrap() {
    logical_queue_equality_and_futures();
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [1, 44_100, 48_000, 192_000] {
            for drained in [false, true] {
                let mut source = active_audio(region);
                source.set_sample_rate(rate);
                let ticks = if rate == 1 {
                    source.master_hz + 97
                } else {
                    (SAMPLE_CAPACITY as u64 + 97) * source.master_hz / u64::from(rate)
                };
                source.advance_to(source.now + ticks);
                source.write_psg(source.now, 0x91);
                source.write_opn2(source.now, 0, 0x2a);
                source.write_opn2(source.now, 1, 0xff);
                source.write_opn2(source.now, 0, 0x2b);
                source.write_opn2(source.now, 1, 0x80);
                if drained {
                    pcm(&mut source);
                } else if rate != 1 {
                    assert_eq!(source.len, SAMPLE_CAPACITY);
                    assert_ne!(source.head, 0);
                }
                assert_ne!(source.sample_phase, 0);
                let hardware = bytes(&source);
                let presentation = frontend(&source);
                let mut target = Audio::new(region);
                target.set_sample_rate(rate);
                let until = source.now + 17_123 + if rate == 1 { source.master_hz } else { 0 };
                source.advance_to(until);
                let expected = pcm(&mut source);
                if rate == 1 {
                    assert_eq!(expected.len(), if drained { 2 } else { 4 });
                }
                target
                    .read_state(&mut Reader::new(&hardware), region)
                    .unwrap();
                target
                    .read_frontend_state(&mut Reader::new(&presentation))
                    .unwrap();
                target.advance_to(until);
                assert_eq!(pcm(&mut target), expected, "{region:?} {rate} {drained}");
                assert_eq!(frontend(&target), frontend(&source));
                assert_eq!(bytes(&target), bytes(&source));
                assert_eq!(target, source);
            }
        }
    }
}

fn logical_queue_equality_and_futures() {
    let mut drained = active_audio(Region::NtscUs);
    let emitted = pcm(&mut drained);
    assert!(emitted.iter().any(|sample| *sample != 0.0));
    assert_eq!(drained.len, 0);
    assert!(
        drained
            .samples
            .iter()
            .any(|sample| *sample != [0; OUTPUT_CHANNELS])
    );
    let mut restored = Audio::new(Region::NtscUs);
    restore(&drained, &mut restored);
    assert_ne!(drained.samples, restored.samples);
    assert_eq!(frontend(&drained), frontend(&restored));
    assert_eq!(drained, restored);

    let mut stale = restored.clone();
    stale.samples[SAMPLE_CAPACITY - 1] = [i16::MIN, i16::MAX];
    assert_eq!(stale, restored);
    let mut wrapped = drained.clone();
    for index in 0..SAMPLE_CAPACITY + 7 {
        wrapped.push([index as i16, -(index as i16)]);
    }
    assert_eq!((wrapped.head, wrapped.len), (7, SAMPLE_CAPACITY));
    restore(&wrapped, &mut restored);
    assert_eq!(restored.head, 0);
    assert_ne!(wrapped.samples, restored.samples);
    assert_eq!(wrapped, restored);

    let mut changed = restored.clone();
    changed.samples[17][1] ^= 1;
    assert_ne!(wrapped, changed);
    let mut reordered = restored.clone();
    reordered.samples.swap(0, 1);
    assert_ne!(wrapped, reordered);
    let mut shorter = restored.clone();
    shorter.len -= 1;
    assert_ne!(wrapped, shorter);
    let mut wrong_origin = wrapped.clone();
    wrong_origin.head = (wrong_origin.head + 1) % SAMPLE_CAPACITY;
    assert_ne!(wrapped, wrong_origin);

    for mutation in 0..8 {
        let mut changed = restored.clone();
        match mutation {
            0 => changed.sample_phase ^= 1,
            1 => changed.chip_phase ^= 1,
            2 => changed.opn2_phase ^= 1,
            3 => changed.generation_enabled = !changed.generation_enabled,
            4 => changed.mutes[0] = !changed.mutes[0],
            5 => changed.fm_mutes[0] = !changed.fm_mutes[0],
            6 => changed.deferred = Some(changed.now),
            _ => changed.output = Output::at_level([i32::MAX; OUTPUT_CHANNELS]),
        }
        assert_ne!(restored, changed);
    }
    let mut traced = restored.clone();
    traced.trace = traced
        .trace
        .prepare_with_clock(
            4,
            zeff_emu_common::time::ClockRate::from_hz(traced.master_hz),
            crate::hardware::audio::trace::trace_chip(Region::NtscUs),
            zeff_emu_common::audio_trace::AudioTraceTiming::BusServiceBoundary,
        )
        .unwrap();
    assert_ne!(restored, traced);
    let mut originated = restored.clone();
    originated.trace_origin = Some(zeff_emu_common::audio_trace::MdTraceOrigin {
        writer: zeff_emu_common::audio_trace::MdTraceWriter::M68000,
        boundary_pc: 0x200,
        source: zeff_emu_common::audio_trace::MdTraceSource::Unknown,
    });
    assert_ne!(restored, originated);

    let now = wrapped.now;
    for at in [now + 1, now + 17, now + 19_123] {
        wrapped.advance_to(at);
        restored.advance_to(at);
        let expected = pcm(&mut wrapped);
        let actual = pcm(&mut restored);
        assert_eq!(
            expected
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
            actual
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(bytes(&wrapped), bytes(&restored));
        assert_eq!(frontend(&wrapped), frontend(&restored));
        assert_eq!(wrapped, restored);
    }
}

#[test]
fn matching_policies_restore_tails_after_each_restart_policy() {
    for policy in 0..5 {
        let mut source = active_audio(Region::NtscUs);
        match policy {
            0 => source.set_sample_rate(44_100),
            1 => source.set_channel_mutes([true, false, true, false]),
            2 => source.set_fm_channel_mutes([false, true, false, true, false, true]),
            3 => source.set_sample_generation_enabled(false),
            4 => {
                source.set_sample_generation_enabled(false);
                source.set_sample_generation_enabled(true);
            }
            _ => unreachable!(),
        }
        source.write_psg(source.now, 0x92);
        source.write_opn2(source.now, 0, 0x2a);
        source.write_opn2(source.now, 1, 0xff);
        source.write_opn2(source.now, 0, 0x2b);
        source.write_opn2(source.now, 1, 0x80);
        let mut target = source.clone();
        let hardware = bytes(&source);
        let presentation = frontend(&source);
        let until = source.now + 41_999;
        source.advance_to(until);
        let expected = pcm(&mut source);
        target
            .read_state(&mut Reader::new(&hardware), Region::NtscUs)
            .unwrap();
        target
            .read_frontend_state(&mut Reader::new(&presentation))
            .unwrap();
        target.advance_to(until);
        assert_eq!(pcm(&mut target), expected);
        assert_eq!(frontend(&target), frontend(&source));
    }
}

#[test]
fn each_policy_mismatch_preserves_destination_and_hardware_restart() {
    let source = active_audio(Region::NtscUs);
    for policy in 0..4 {
        let mut target = source.clone();
        match policy {
            0 => target.set_sample_rate(1),
            1 => target.set_sample_generation_enabled(false),
            2 => target.set_channel_mutes([true; PSG_CHANNELS]),
            3 => target.set_fm_channel_mutes([true; FM_CHANNEL_COUNT]),
            _ => unreachable!(),
        }
        let mut expected = target.clone();
        expected
            .read_state(&mut Reader::new(&bytes(&source)), Region::NtscUs)
            .unwrap();
        restore(&source, &mut target);
        assert_eq!(target, expected);
        target.advance_to(source.now + 19_123);
        expected.advance_to(source.now + 19_123);
        assert_eq!(pcm(&mut target), pcm(&mut expected));
    }
}

#[test]
fn canonical_capture_never_advances_a_deferred_batch() {
    let mut source = active_audio(Region::NtscUs);
    source.begin_batch();
    source.request_advance_to(source.now + 17);
    let before = source.clone();
    let result = std::panic::catch_unwind(|| frontend(&source));
    assert!(result.is_err());
    assert_eq!(source, before);
}

use super::*;

fn active(region: Region, rate: u32) -> Emulator {
    let mut source = super::super::cpu_continuation_tests::machine(region, false);
    source.set_sample_rate(rate);
    for _ in 0..20_000 {
        if source.sound_cpu.cycles() >= 400 {
            let now = source.bus.now.get();
            for value in [0x81, 0x02, 0x90, 0xe7, 0xf2] {
                source.bus.audio.write_psg(now, value);
            }
            return source;
        }
        source.step_instruction().unwrap();
    }
    panic!("sound program did not start");
}

fn audio_start(saved: &[u8]) -> usize {
    let mut reader = StateReader::new(saved);
    reader.set_position(12);
    reader.read_slice(MAX_STATE_BYTES).unwrap();
    reader.position() + 4
}

#[test]
fn concurrent_field_boundary_matches_uninterrupted_pcm_hardware_and_video() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [44_100, 48_000, 192_000] {
            for drained in [false, true] {
                let mut source = active(region, rate);
                source.step_field().unwrap();
                if drained {
                    source.drain_audio_samples();
                }
                let saved = source.encode_frontend_state().unwrap();
                let raw = source.encode_state().unwrap();
                let mut reader = StateReader::new(&saved);
                reader.set_position(12);
                assert_eq!(reader.read_slice(MAX_STATE_BYTES).unwrap(), raw);
                assert_eq!(
                    u32::from_le_bytes(raw[8..12].try_into().unwrap()),
                    crate::save_state::STATE_VERSION
                );
                assert!(saved.len() <= MAX_FRONTEND_STATE_BYTES);
                let mut restored = source.clone();
                let mut expected = Vec::new();
                for _ in 0..2 {
                    source.step_field().unwrap();
                    expected.push((
                        source.drain_audio_samples(),
                        source.encode_state().unwrap(),
                        source.framebuffer().to_vec(),
                    ));
                }
                restored.decode_frontend_state(&saved).unwrap();
                assert_eq!(restored.encode_frontend_state().unwrap(), saved);
                for (pcm, state, video) in expected {
                    restored.step_field().unwrap();
                    assert!(!pcm.is_empty());
                    assert!(pcm.iter().any(|sample| *sample != 0.0));
                    assert_eq!(restored.drain_audio_samples(), pcm);
                    assert_eq!(restored.encode_state().unwrap(), state);
                    assert_eq!(restored.framebuffer(), video);
                }
            }
        }
    }
}

#[test]
fn legacy_magic_uses_existing_hardware_reset_contract() {
    let source = active(Region::NtscUs, 48_000);
    let raw = source.encode_state().unwrap();
    let mut target = source.clone();
    target.set_sample_rate(44_100);
    target.set_apu_channel_mutes([true, false, false, true]);
    let mut expected = target.clone();
    expected.decode_state(&raw).unwrap();
    target.decode_frontend_state(&raw).unwrap();
    assert_eq!(target, expected);
    assert_eq!(target.encode_state().unwrap(), raw);
    assert!(target.encode_frontend_state().unwrap().starts_with(MAGIC));
}

#[test]
fn malformed_envelope_and_late_audio_fields_preserve_frontend_and_future_pcm() {
    let source = active(Region::NtscUs, 48_000);
    let saved = source.encode_frontend_state().unwrap();
    let at = audio_start(&saved);
    assert_ne!(
        u32::from_le_bytes(saved[at + 600..at + 604].try_into().unwrap()),
        0
    );
    let mut rejects = vec![saved[..12].to_vec(), saved[..saved.len() - 1].to_vec()];
    for (offset, replacement) in [
        (0, vec![b'X']),
        (8, 2_u32.to_le_bytes().to_vec()),
        (12, u32::MAX.to_le_bytes().to_vec()),
        (at - 4, u32::MAX.to_le_bytes().to_vec()),
        (at, 0_u32.to_le_bytes().to_vec()),
        (at, 192_001_u32.to_le_bytes().to_vec()),
        (at + 4, vec![2]),
        (at + 4, vec![0]),
        (at + 5, vec![0x80]),
        (at + 6, vec![0x80]),
        (at + 7, 1_u64.to_le_bytes().to_vec()),
        (at + 15, u64::MAX.to_le_bytes().to_vec()),
        (at + 23, region_master(&source).to_le_bytes().to_vec()),
        (at + 31, i64::MAX.to_le_bytes().to_vec()),
        (at + 575, i64::MIN.to_le_bytes().to_vec()),
        (at + 591, 32_768_i32.to_le_bytes().to_vec()),
        (at + 599, vec![34]),
        (at + 600, 4097_u32.to_le_bytes().to_vec()),
    ] {
        let mut bad = saved.clone();
        bad[offset..offset + replacement.len()].copy_from_slice(&replacement);
        rejects.push(bad);
    }
    let mut trailing = saved.clone();
    trailing.push(0);
    rejects.push(trailing);
    let mut late_trailing = saved.clone();
    let audio_len = saved.len() - at;
    late_trailing[at - 4..at].copy_from_slice(&((audio_len + 1) as u32).to_le_bytes());
    late_trailing.push(0);
    rejects.push(late_trailing);
    let mut wrong_level = saved.clone();
    let level = i32::from_le_bytes(saved[at + 591..at + 595].try_into().unwrap());
    let level: i32 = if level == 0 { 1 } else { 0 };
    for slot in 0..34 {
        wrong_level[at + 31 + slot * 16..at + 39 + slot * 16].fill(0);
    }
    wrong_level[at + 575..at + 583].copy_from_slice(&(i64::from(level) * (1 << 22)).to_le_bytes());
    wrong_level[at + 591..at + 595].copy_from_slice(&level.to_le_bytes());
    assert!(
        source
            .clone()
            .decode_frontend_state(&wrong_level)
            .unwrap_err()
            .to_string()
            .contains("output level mismatch")
    );
    rejects.push(wrong_level);
    for mismatched in [false, true] {
        let mut original = source.clone();
        if mismatched {
            original.set_sample_rate(44_100);
        }
        for bad in &rejects {
            let mut target = original.clone();
            let before = target.encode_frontend_state().unwrap();
            assert!(target.decode_frontend_state(bad).is_err());
            assert_eq!(target.encode_frontend_state().unwrap(), before);
            assert_eq!(target, original);
            let mut control = original.clone();
            for _ in 0..256 {
                assert_eq!(target.step_instruction(), control.step_instruction());
            }
            assert_eq!(target.drain_audio_samples(), control.drain_audio_samples());
            assert_eq!(
                target.encode_frontend_state().unwrap(),
                control.encode_frontend_state().unwrap()
            );
        }
    }
}

fn region_master(source: &Emulator) -> u64 {
    source.region().master_hz()
}

#[test]
fn maximum_includes_complete_queue_and_only_the_optional_api_accepts_envelopes() {
    assert_eq!(MAX_FRONTEND_AUDIO_BYTES, 16_988);
    assert_eq!(MAX_FRONTEND_STATE_BYTES, MAX_STATE_BYTES + 17_008);
    let mut source = active(Region::NtscUs, 192_000);
    source.step_field().unwrap();
    source.step_field().unwrap();
    let saved = source.encode_frontend_state().unwrap();
    let at = audio_start(&saved);
    assert_eq!(saved.len() - at, MAX_FRONTEND_AUDIO_BYTES);
    assert!(source.decode_state(&saved).is_err());
    let mut oversized = saved;
    oversized.resize(MAX_FRONTEND_STATE_BYTES + 1, 0);
    assert!(source.decode_frontend_state(&oversized).is_err());
}

#[test]
fn tagged_prefetch_frontend_roundtrip_preserves_next_read_and_field_continuation() {
    use super::super::state::tests as states;
    use crate::hardware::bus::Prefetch;
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            for history in [
                Prefetch::Empty,
                Prefetch::UpperOnly(0xa7),
                Prefetch::Full(0x01a5),
            ] {
                let mut source = states::machine(region, concurrent);
                source.bus.prefetch = history;
                let saved = source.encode_frontend_state().unwrap();
                assert_eq!(&saved[8..12], &1_u32.to_le_bytes());
                let mut target = states::machine(region, concurrent);
                target.decode_frontend_state(&saved).unwrap();
                assert_eq!(target.bus.prefetch, history);
                assert_eq!(target.encode_frontend_state().unwrap(), saved);
                states::prefetch::assert_reads(&source, history);
                states::prefetch::assert_reads(&target, history);
                source.step_field().unwrap();
                target.step_field().unwrap();
                assert_eq!(target.framebuffer(), source.framebuffer());
                assert_eq!(target.drain_audio_samples(), source.drain_audio_samples());
                assert_eq!(
                    target.encode_frontend_state().unwrap(),
                    source.encode_frontend_state().unwrap()
                );
            }
        }
    }
}

#[test]
fn prefetch_and_late_frontend_failures_preserve_full_live_history_and_future_output() {
    use super::super::state::tests::{layout, machine};
    use crate::hardware::bus::Prefetch;
    let mut source = machine(Region::NtscUs, true);
    source.bus.prefetch = Prefetch::Full(0xfe5a);
    let saved = source.encode_frontend_state().unwrap();
    let raw = source.encode_state().unwrap();
    let at = layout::prefetch_offset(&raw);
    let mut bad_raws = Vec::new();
    let mut bad_tag = raw.clone();
    bad_tag[at] = 3;
    bad_raws.push(bad_tag);
    for length in at..=at + 2 {
        bad_raws.push(raw[..length].to_vec());
    }
    let mut bad_version = raw.clone();
    bad_version[8..12].copy_from_slice(&(crate::save_state::STATE_VERSION + 1).to_le_bytes());
    bad_raws.push(bad_version);
    let audio_at = audio_start(&saved);
    let mut rejects = Vec::new();
    for raw in bad_raws {
        let mut writer = StateWriter::new();
        writer.write_bytes(&saved[..12]);
        writer.write_vec(&raw);
        writer.write_vec(&saved[audio_at..]);
        rejects.push(writer.into_bytes());
    }
    let mut late = saved.clone();
    late[audio_at + 599] = 34;
    rejects.push(late);
    let mut original = source.clone();
    original.bus.prefetch = Prefetch::Full(0x4e71);
    let before = original.encode_frontend_state().unwrap();
    for bad in rejects {
        let mut target = original.clone();
        assert!(target.decode_frontend_state(&bad).is_err());
        assert_eq!(target, original);
        assert_eq!(target.bus.prefetch, Prefetch::Full(0x4e71));
        assert_eq!(target.encode_frontend_state().unwrap(), before);
        let mut control = original.clone();
        target.step_field().unwrap();
        control.step_field().unwrap();
        assert_eq!(target.framebuffer(), control.framebuffer());
        assert_eq!(target.drain_audio_samples(), control.drain_audio_samples());
        assert_eq!(
            target.encode_state().unwrap(),
            control.encode_state().unwrap()
        );
    }
}

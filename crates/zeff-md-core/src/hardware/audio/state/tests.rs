use super::*;

fn bytes(audio: &Audio) -> Vec<u8> {
    let mut w = Writer::new();
    audio.write_state(&mut w).unwrap();
    w.into_bytes()
}

fn fm_write(audio: &mut Audio, address: u8, value: u8) {
    audio.write_opn2(0, 0, address);
    audio.write_opn2(0, 1, value);
}

fn active_audio(region: Region) -> Audio {
    let mut audio = Audio::new(region);
    for value in [0x81, 0x02, 0x90, 0xe7, 0xf2] {
        audio.write_psg(0, value);
    }
    for slot in [0, 4, 8, 12] {
        fm_write(&mut audio, 0x30 + slot, 1);
        fm_write(&mut audio, 0x50 + slot, 31);
        fm_write(&mut audio, 0x60 + slot, 0x8c);
        fm_write(&mut audio, 0x80 + slot, 0x47);
    }
    for (address, value) in [
        (0xb0, 0x3f),
        (0xb4, 0xf6),
        (0x22, 13),
        (0xa4, 0x22),
        (0xa0, 0x69),
        (0x28, 0xf0),
        (0x24, 0xff),
        (0x25, 3),
        (0x26, 0xff),
        (0x27, 15),
    ] {
        fm_write(&mut audio, address, value);
    }
    audio.advance_to(321_987);
    audio
}

#[test]
fn hardware_continuation_preserves_synthesis_and_fractional_clocks() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut source = active_audio(region);
        assert!(source.len != 0);
        let saved = bytes(&source);
        let mut restored = Audio::new(region);
        restored
            .read_state(&mut Reader::new(&saved), region)
            .unwrap();
        assert_eq!(bytes(&restored), saved);
        source.restart_output();
        for at in [322_001, 325_678, 700_003] {
            source.advance_to(at);
            restored.advance_to(at);
            assert_eq!(source, restored);
        }
        let mut pcm = Vec::new();
        restored.drain_into(&mut pcm);
        assert!(pcm.iter().any(|sample| *sample != 0.0));
    }
}

#[test]
fn host_audio_policy_is_excluded_and_target_settings_survive() {
    let source = active_audio(Region::NtscUs);
    let saved = bytes(&source);
    let mut altered = source.clone();
    altered.set_sample_rate(32_000);
    altered.set_sample_generation_enabled(false);
    altered.set_channel_mutes([true; PSG_CHANNELS]);
    altered.set_fm_channel_mutes([true; FM_CHANNEL_COUNT]);
    assert_eq!(bytes(&altered), saved);
    altered
        .read_state(&mut Reader::new(&saved), Region::NtscUs)
        .unwrap();
    assert_eq!(altered.sample_rate, 32_000);
    assert!(!altered.generation_enabled);
    assert_eq!(altered.mutes, [true; PSG_CHANNELS]);
    assert_eq!(altered.fm_mutes, [true; FM_CHANNEL_COUNT]);
    assert_eq!(altered.sample_phase, 0);
    assert_eq!(altered.len, 0);
    assert!(
        altered
            .samples
            .iter()
            .all(|sample| *sample == [0; OUTPUT_CHANNELS])
    );
}

#[test]
fn malformed_audio_is_transactional_including_late_phases() {
    let original = active_audio(Region::NtscUs);
    let saved = bytes(&original);
    let mut mutations = Vec::new();
    let mut bad = saved.clone();
    bad[6] = 16;
    mutations.push(bad);
    let mut bad = saved.clone();
    let phase = bad.len() - 8;
    bad[phase..].copy_from_slice(&OPN2_CLOCK_TICKS.to_le_bytes());
    mutations.push(bad);
    for len in [0, 1, 27, saved.len() - 1] {
        mutations.push(saved[..len].to_vec());
    }
    for bad in mutations {
        let mut target = original.clone();
        assert!(
            target
                .read_state(&mut Reader::new(&bad), Region::NtscUs)
                .is_err()
        );
        assert_eq!(target, original);
    }
}

#[test]
fn changed_period_and_timer_reload_keep_old_countdown() {
    let mut audio = Audio::default();
    for value in [0x8f, 0x3f] {
        audio.write_psg(0, value);
    }
    fm_write(&mut audio, 0x27, 1);
    audio.advance_to(1_000);
    audio.write_psg(1_000, 0x81);
    audio.write_psg(1_000, 0);
    audio.write_opn2(1_000, 0, 0x24);
    audio.write_opn2(1_000, 1, 0xff);
    let saved = bytes(&audio);
    let mut restored = Audio::default();
    restored
        .read_state(&mut Reader::new(&saved), Region::NtscUs)
        .unwrap();
    assert_eq!(bytes(&restored), saved);
    assert!(audio.psg.debug_snapshot().counters[0] > audio.psg.debug_snapshot().periods[0]);
}

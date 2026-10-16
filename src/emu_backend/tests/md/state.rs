use super::*;
use crate::emu_core_trait::EmulatorCore;
use zeff_emu_common::StateRestoreOutcome;

fn visible_audio_rom(header: u8) -> Vec<u8> {
    let mut words = Vec::new();
    for (address, value) in [
        (0xa11100, 1),
        (0xa11200, 1),
        (0xa04002, 0xb6),
        (0xa04003, 0x80),
        (0xa04000, 0x2a),
        (0xa04001, 0xff),
        (0xa04000, 0x2b),
        (0xa04001, 0x80),
    ] {
        write_byte(&mut words, address, value);
    }
    write_word(&mut words, VDP_CONTROL, 0x8c81);
    write_word(
        &mut words,
        VDP_CONTROL,
        if header == b'E' { 0x810c } else { 0x8104 },
    );
    write_word(&mut words, VDP_CONTROL, 0xc000);
    write_word(&mut words, VDP_CONTROL, 0);
    write_word(&mut words, 0xc00000, 0x000e);
    words.extend(LOOP);
    rom(header, &words)
}

fn new_visible_audio(header: u8, sample_rate: u32) -> EmuBackend {
    load(
        visible_audio_rom(header),
        BackendLoadConfig {
            sample_rate: Some(sample_rate),
            ..BackendLoadConfig::default()
        },
    )
}

#[test]
fn md_backend_state_restores_visible_frame_and_future_stereo_pcm() {
    for header in *b"UEJ" {
        let mut control = new_visible_audio(header, 48_000);
        control.step_frame();
        assert!(!control.is_suspended());
        assert_eq!(machine(&control).video_error(), None);
        let frame = control.framebuffer().to_vec();
        assert!(
            frame
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0)
        );
        let dimensions = control.display_framebuffer().1;
        assert_eq!(
            dimensions,
            Some((320, if header == b'E' { 240 } else { 224 }))
        );
        let mut initial_pcm = Vec::new();
        control.drain_audio_samples_into(&mut initial_pcm);
        assert!(initial_pcm.iter().any(|sample| *sample != 0.0));
        let saved = control.encode_state_bytes().unwrap();
        let mut restored = new_visible_audio(header, 48_000);
        restored.step_frame();
        restored.step_frame();
        assert_eq!(
            restored.load_state_from_bytes(saved.clone()).unwrap(),
            StateRestoreOutcome::Exact
        );
        assert_eq!(restored.framebuffer(), frame);
        assert_eq!(restored.display_framebuffer().1, dimensions);
        assert!(restored.rewind_framebuffer().is_empty());
        assert_eq!(restored.encode_state_bytes().unwrap(), saved);
        let mut drained = Vec::new();
        restored.drain_audio_samples_into(&mut drained);
        assert!(drained.is_empty());
        control.set_sample_rate(48_000);
        for _ in 0..2 {
            control.step_frame();
            restored.step_frame();
            assert!(!control.is_suspended() && !restored.is_suspended());
            assert_eq!(restored.frame_count(), control.frame_count());
            assert_eq!(restored.framebuffer(), control.framebuffer());
            assert_eq!(
                restored.encode_state_bytes().unwrap(),
                control.encode_state_bytes().unwrap()
            );
            let mut expected = Vec::new();
            let mut actual = Vec::new();
            control.drain_audio_samples_into(&mut expected);
            restored.drain_audio_samples_into(&mut actual);
            assert!(!expected.is_empty() && expected.iter().any(|sample| *sample != 0.0));
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn md_backend_bad_identity_and_late_payload_are_transactional() {
    let mut target = new_visible_audio(b'U', 48_000);
    target.step_frame();
    let saved = target.encode_state_bytes().unwrap();
    target.step_frame();
    let before = machine(&target).clone();
    let mut corrupt = saved.clone();
    *corrupt.last_mut().unwrap() = 2;
    let mut trailing = saved.clone();
    trailing.push(0);
    let mut other_rom = visible_audio_rom(b'U');
    other_rom[0x180] ^= 1;
    let wrong_rom = load(other_rom, BackendLoadConfig::default())
        .encode_state_bytes()
        .unwrap();
    let wrong_region = new_visible_audio(b'E', 48_000)
        .encode_state_bytes()
        .unwrap();
    let wrong_controllers = load(
        visible_audio_rom(b'U'),
        BackendLoadConfig {
            md_controller: ControllerKind::ThreeButton,
            ..BackendLoadConfig::default()
        },
    )
    .encode_state_bytes()
    .unwrap();
    for bad in [
        saved[..12].to_vec(),
        saved[..saved.len() - 1].to_vec(),
        corrupt,
        trailing,
        wrong_rom,
        wrong_region,
        wrong_controllers,
    ] {
        assert!(target.load_state_from_bytes(bad).is_err());
        assert_eq!(machine(&target), &before);
        assert!(!target.is_suspended());
        assert!(target.take_runtime_fault().is_none());
    }
}

#[test]
fn md_backend_restore_recovers_suspend_and_fault_only_after_success() {
    let mut backend = load(rom(b'U', &LOOP), BackendLoadConfig::default());
    backend.step_frame();
    let saved = backend.encode_state_bytes().unwrap();
    let EmuBackend::Md(md) = &mut backend else {
        unreachable!()
    };
    md.debug_suspend();
    let before = machine(&backend).clone();
    assert!(backend.encode_state_bytes().is_err());
    assert!(backend.load_state_from_bytes(vec![0; 64]).is_err());
    assert_eq!(machine(&backend), &before);
    assert!(backend.is_suspended());
    backend.load_state_from_bytes(saved.clone()).unwrap();
    assert!(!backend.is_suspended());
    assert!(backend.take_runtime_fault().is_none());
    backend.step_frame();
    assert!(!backend.is_suspended());

    let mut words = Vec::new();
    write_byte(&mut words, 0xb00000, 0xaa);
    words.extend(LOOP);
    let mut faulted = load(rom(b'U', &words), BackendLoadConfig::default());
    let recover = faulted.encode_state_bytes().unwrap();
    faulted.step_frame();
    assert!(faulted.is_suspended());
    let before = machine(&faulted).clone();
    assert!(faulted.encode_state_bytes().is_err());
    assert!(faulted.load_state_from_bytes(saved).is_err());
    assert_eq!(machine(&faulted), &before);
    assert!(faulted.is_suspended());
    assert!(
        faulted
            .take_runtime_fault()
            .unwrap()
            .contains("machine fault")
    );
    faulted.reset();
    faulted.step_frame();
    faulted.load_state_from_bytes(recover.clone()).unwrap();
    assert!(!faulted.is_suspended());
    assert!(faulted.take_runtime_fault().is_none());
    assert_eq!(faulted.encode_state_bytes().unwrap(), recover);
}

#[test]
fn md_backend_state_preserves_target_audio_policy() {
    let mut source = new_visible_audio(b'U', 44_100);
    source.step_frame();
    let saved = source.encode_state_bytes().unwrap();
    let mut target = new_visible_audio(b'U', 96_000);
    target.set_apu_sample_generation_enabled(false);
    target.set_apu_channel_mutes(&[true; 10]);
    target.step_frame();
    assert_eq!(target.encode_state_bytes().unwrap(), saved);
    target.load_state_from_bytes(saved).unwrap();
    assert_eq!(machine(&target).sample_rate(), 96_000);
    target.step_frame();
    let mut samples = Vec::new();
    target.drain_audio_samples_into(&mut samples);
    assert!(samples.is_empty());
    target.set_apu_sample_generation_enabled(true);
    target.step_frame();
    target.drain_audio_samples_into(&mut samples);
    assert!(!samples.is_empty());
    assert!(samples.iter().all(|sample| *sample == 0.0));
}

#[test]
fn md_backend_direct_trait_admits_only_ordinary_save_state_capability() {
    let backend = load(rom(b'U', &LOOP), BackendLoadConfig::default());
    let EmuBackend::Md(backend) = backend else {
        unreachable!()
    };
    assert!(backend.supports_save_states() && backend.supports_state_capture());
    assert!(backend.state_restores_framebuffer());
    assert!(!backend.supports_rewind() && !backend.supports_replay());
}

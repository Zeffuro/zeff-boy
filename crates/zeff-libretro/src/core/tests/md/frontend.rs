use super::*;

#[test]
fn md_legacy_hardware_payload_retains_existing_restart_semantics() {
    let mut state = CoreState::from_rom(&guest(), "legacy.md").unwrap();
    state.step_frame();
    assert_eq!(state.take_runtime_fault(), None);
    let raw = machine(&state).encode_state().unwrap();
    let mut expected = machine(&state).clone();
    expected.decode_state(&raw).unwrap();
    state.load_state(&raw).unwrap();
    assert_eq!(
        state.encode_state().unwrap(),
        expected.encode_frontend_state().unwrap()
    );
    state.step_frame();
    expected.step_field().unwrap();
    state.drain_audio();
    assert_eq!(state.audio_buf, expected.drain_audio_samples());
}

fn md_continuation_pcm(state: &mut CoreState) -> Vec<Vec<i16>> {
    let mut frames = Vec::new();
    for frame in 4..7 {
        state.set_input(frame, frame ^ 3);
        state.set_input_p2(frame ^ 1, frame ^ 2);
        state.step_frame();
        assert_eq!(state.take_runtime_fault(), None);
        state.drain_audio();
        assert!(!state.audio_buf.is_empty());
        assert!(state.audio_buf.len().is_multiple_of(2));
        assert!(state.audio_buf.iter().all(|sample| sample.is_finite()));
        frames.push(
            state
                .audio_buf
                .iter()
                .map(|&sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .collect(),
        );
    }
    frames
}

#[test]
fn md_frontend_restore_matches_uninterrupted_pcm() {
    for region in *b"UEJ" {
        let mut bytes = guest();
        bytes[0x1f0] = region;
        let mut state = CoreState::from_rom(&bytes, "continuation.md").unwrap();
        assert_eq!(state.sample_rate, 48_000);
        assert_eq!(machine(&state).sample_rate(), 48_000);
        for frame in 0..4 {
            state.set_input(frame, frame ^ 3);
            state.set_input_p2(frame ^ 1, frame ^ 2);
            state.step_frame();
            assert_eq!(state.take_runtime_fault(), None);
            state.drain_audio();
        }
        let checkpoint = state.encode_state().unwrap();
        let checkpoint_hardware = machine(&state).encode_state().unwrap();

        // Observe the control before any restore can restart its presentation.
        let expected_pcm = md_continuation_pcm(&mut state);
        assert!(expected_pcm.iter().flatten().any(|&sample| sample != 0));
        let expected_final = state.encode_state().unwrap();
        let expected_hardware = machine(&state).encode_state().unwrap();
        let expected_save_ram = state.battery_sram();

        state.load_state(&checkpoint).unwrap();
        assert_eq!(state.encode_state().unwrap(), checkpoint);
        assert_eq!(machine(&state).encode_state().unwrap(), checkpoint_hardware);
        let actual_pcm = md_continuation_pcm(&mut state);
        assert_eq!(machine(&state).encode_state().unwrap(), expected_hardware);
        assert_eq!(state.battery_sram(), expected_save_ram);
        for (frame, (actual, expected)) in actual_pcm.iter().zip(&expected_pcm).enumerate() {
            assert_eq!(
                actual.len(),
                expected.len(),
                "region={region} frame={frame}"
            );
            let first_difference = actual.iter().zip(expected).position(|(a, b)| a != b);
            assert_eq!(first_difference, None, "region={region} frame={frame}");
        }
        assert_eq!(state.encode_state().unwrap(), expected_final);
    }
}

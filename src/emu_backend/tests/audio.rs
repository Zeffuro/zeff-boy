use super::*;

fn assert_audio_topology_contract(mut backend: EmuBackend, expected_channels: usize) {
    let before = backend
        .audio_topology()
        .expect("audio-capable backend should expose a topology");
    assert!(before.generation > 0);
    assert_eq!(before.channels.len(), expected_channels);
    let ids = before
        .channels
        .iter()
        .map(|channel| channel.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), before.channels.len());
    assert!(
        before
            .channels
            .iter()
            .all(|channel| !channel.name.is_empty() && !channel.group.is_empty())
    );

    backend.step_frame();
    let after = backend
        .audio_topology()
        .expect("audio topology should remain available after stepping");
    assert_eq!(after, before);
    let frame = backend
        .audio_semantic_frame()
        .expect("audio topology should have semantic state");
    assert_eq!(frame.voices.len(), expected_channels);
    let frame_ids = frame
        .voices
        .iter()
        .map(|voice| voice.channel)
        .collect::<BTreeSet<_>>();
    assert_eq!(frame_ids, ids);
    for voice in frame.voices {
        let descriptor = before
            .channels
            .iter()
            .find(|channel| channel.id == voice.channel)
            .expect("semantic voice ID should resolve in the topology");
        if descriptor.class != crate::audio_tooling::AudioVoiceClass::Other {
            assert_eq!(descriptor.class, voice.class);
        }
        assert!(
            descriptor
                .caps
                .contains(crate::audio_tooling::AudioSemanticCaps::GATE)
        );
        if !descriptor
            .caps
            .contains(crate::audio_tooling::AudioSemanticCaps::PITCH)
        {
            assert_eq!(voice.pitch_hz, None);
        }
        if voice.level.is_some() {
            assert!(
                descriptor
                    .caps
                    .contains(crate::audio_tooling::AudioSemanticCaps::LEVEL)
            );
        }
    }
}

#[test]
fn every_audio_backend_exposes_a_stable_topology_contract() {
    assert_audio_topology_contract(build_gb_backend(), 4);
    assert_audio_topology_contract(build_gba_backend(), 6);
    assert_audio_topology_contract(build_nes_backend(), 5);
    assert_audio_topology_contract(build_coleco_backend(), 4);
    assert_audio_topology_contract(build_pce_backend(), 6);
    assert_audio_topology_contract(build_sms_backend(), 4);
    assert_audio_topology_contract(build_ws_backend(), 5);
}

#[test]
fn pce_audio_semantics_keep_zero_pitch_and_wave_noise_identity() {
    let backend = build_pce_backend();
    let topology = backend.audio_topology().unwrap();
    assert_eq!(
        topology.channels[4].class,
        crate::audio_tooling::AudioVoiceClass::WavetableNoise
    );
    assert_eq!(
        topology.channels[5].class,
        crate::audio_tooling::AudioVoiceClass::WavetableNoise
    );

    let frame = backend.audio_semantic_frame().unwrap();
    let expected = (zeff_pce_core::hardware::PSG_CLOCK_NUMERATOR as f64
        / zeff_pce_core::hardware::PSG_CLOCK_DENOMINATOR as f64)
        / (4096.0 * 32.0);
    assert!((frame.voices[0].pitch_hz.unwrap() - expected).abs() < 1e-9);
}

#[test]
fn nes_topology_and_semantic_frame_include_dmc() {
    let mut backend = build_nes_backend();
    let topology = backend.audio_topology().unwrap();
    let dmc = &topology.channels[4];

    assert_eq!(dmc.id, crate::audio_tooling::AudioChannelId(4));
    assert_eq!(dmc.name, "NES DMC");
    assert_eq!(dmc.class, crate::audio_tooling::AudioVoiceClass::Pcm);
    assert_eq!(
        dmc.caps,
        crate::audio_tooling::AudioSemanticCaps::GATE_LEVEL
    );
    assert!(dmc.muteable);

    backend.step_frame();
    let frame = backend
        .audio_semantic_frame()
        .expect("NES should expose semantic audio data for recording/tooling");
    let voice = &frame.voices[4];
    assert_eq!(voice.channel, dmc.id);
    assert_eq!(voice.name, dmc.name);
    assert_eq!(voice.class, dmc.class);
    assert!(!voice.active);
    assert_eq!(voice.pitch_hz, None);
    assert_eq!(voice.level, Some(0.0));
}

#[test]
fn wonder_swan_topology_keeps_hybrid_channel_identities_stable() {
    let topology = build_ws_backend().audio_topology().unwrap();
    assert_eq!(topology.channels[1].name, "WS CH1 Wave/Voice");
    assert_eq!(topology.channels[3].name, "WS CH3 Wave/Noise");
    assert_eq!(
        topology.channels[1].class,
        crate::audio_tooling::AudioVoiceClass::Other
    );
    assert_eq!(
        topology.channels[3].class,
        crate::audio_tooling::AudioVoiceClass::Other
    );
    assert!(
        topology.channels[1]
            .caps
            .contains(crate::audio_tooling::AudioSemanticCaps::PITCH)
    );
    assert!(
        topology.channels[3]
            .caps
            .contains(crate::audio_tooling::AudioSemanticCaps::PITCH)
    );
    assert!(!topology.channels[4].muteable);
    assert!(
        !topology.channels[4]
            .caps
            .contains(crate::audio_tooling::AudioSemanticCaps::PITCH)
    );
}

#[test]
fn sega8_backend_exposes_semantic_audio_frame_for_recording() {
    let mut backend = build_sms_backend();
    backend.step_frame();

    let frame = backend
        .audio_semantic_frame()
        .expect("Sega 8-bit should expose PSG semantic audio data for recording");
    assert_eq!(frame.voices.len(), 4);
    assert_eq!(frame.voices[0].name, "Sega PSG Tone 0");
    assert_eq!(
        frame.voices[3].class,
        crate::audio_tooling::AudioVoiceClass::Noise
    );
    assert_eq!(frame.voices[3].pitch_hz, None);
}

#[test]
fn coleco_backend_exposes_semantic_audio_frame_for_recording() {
    let mut backend = build_coleco_backend();
    backend.step_frame();

    let frame = backend
        .audio_semantic_frame()
        .expect("ColecoVision should expose PSG semantic audio data for recording");
    assert_eq!(frame.voices.len(), 4);
    assert_eq!(frame.voices[0].name, "Coleco PSG Tone 0");
    assert_eq!(
        frame.voices[3].class,
        crate::audio_tooling::AudioVoiceClass::Noise
    );
    assert_eq!(frame.voices[3].pitch_hz, None);
}

#[test]
fn gba_backend_exposes_semantic_audio_frame_for_recording() {
    let mut backend = build_gba_backend();
    backend.step_frame();

    let frame = backend
        .audio_semantic_frame()
        .expect("GBA should expose PSG and FIFO semantic audio data for recording/tooling");
    assert_eq!(frame.voices.len(), 6);
    assert_eq!(frame.voices[0].name, "GBA PSG 1 (Square + Sweep)");
    assert_eq!(
        frame.voices[2].class,
        crate::audio_tooling::AudioVoiceClass::Wavetable
    );
    assert_eq!(
        frame.voices[3].class,
        crate::audio_tooling::AudioVoiceClass::Noise
    );
    assert_eq!(frame.voices[3].pitch_hz, None);
    assert_eq!(
        frame.voices[4].class,
        crate::audio_tooling::AudioVoiceClass::Pcm
    );
    assert_eq!(frame.voices[4].name, "GBA FIFO A");
    assert_eq!(frame.voices[4].pitch_hz, None);
}

#[test]
fn ws_backend_exposes_semantic_audio_frame_for_recording() {
    let mut backend = build_ws_backend();
    backend.step_frame();

    let frame = backend.audio_semantic_frame().expect(
        "WonderSwan should expose wave/noise/PCM semantic audio data for recording/tooling",
    );
    assert_eq!(frame.voices.len(), 5);
    assert_eq!(frame.voices[0].name, "WS CH0 Wave");
    assert_eq!(
        frame.voices[0].class,
        crate::audio_tooling::AudioVoiceClass::Wavetable
    );
    assert_eq!(
        frame.voices[4].class,
        crate::audio_tooling::AudioVoiceClass::Pcm
    );
    assert_eq!(frame.voices[4].name, "WS HyperVoice");
    assert_eq!(frame.voices[4].pitch_hz, None);
}

use super::*;

#[test]
fn gb_backend_smoke_roundtrip() {
    let mut backend = build_gb_backend();

    assert_eq!(backend.system(), ActiveSystem::GameBoy);
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::GameBoy.framebuffer_len()
    );
    assert!(backend.is_running());

    backend.step_frame();

    let state = backend
        .encode_state_bytes()
        .expect("GB backend should encode save-state");
    backend
        .load_state_from_bytes(state)
        .expect("GB backend should load save-state");
}

#[test]
fn nes_backend_smoke_roundtrip() {
    let mut backend = build_nes_backend();

    assert_eq!(backend.system(), ActiveSystem::Nes);
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::Nes.framebuffer_len()
    );
    assert!(backend.is_running());

    backend.step_frame();

    let state = backend
        .encode_state_bytes()
        .expect("NES backend should encode save-state");
    backend
        .load_state_from_bytes(state)
        .expect("NES backend should load save-state");
}

#[test]
fn gba_backend_smoke_roundtrip() {
    let mut backend = build_gba_backend();

    assert_eq!(backend.system(), ActiveSystem::GameBoyAdvance);
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::GameBoyAdvance.framebuffer_len()
    );
    assert!(backend.is_running());

    backend.step_frame();

    let state = backend
        .encode_state_bytes()
        .expect("GBA backend should encode save-state");
    backend
        .load_state_from_bytes(state)
        .expect("GBA backend should load save-state");
}

#[test]
fn ws_backend_smoke_roundtrip() {
    let mut backend = build_ws_backend();

    assert_eq!(backend.system(), ActiveSystem::WonderSwan);
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::WonderSwan.framebuffer_len()
    );
    assert!(backend.is_running());

    backend.step_frame();

    let state = backend
        .encode_state_bytes()
        .expect("WonderSwan backend should encode save-state");
    backend
        .load_state_from_bytes(state)
        .expect("WonderSwan backend should load save-state");
}

#[test]
fn sega8_backend_smoke_roundtrip() {
    let mut backend = build_sms_backend();

    assert_eq!(backend.system(), ActiveSystem::MasterSystem);
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::MasterSystem.framebuffer_len()
    );
    assert!(backend.is_running());

    backend.step_frame();

    let state = backend
        .encode_state_bytes()
        .expect("Sega 8-bit backend should encode save-state");
    backend
        .load_state_from_bytes(state)
        .expect("Sega 8-bit backend should load save-state");
}

#[test]
fn gb_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_gb_backend(), 1, 2);
}

#[test]
fn gb_rtc_replay_hash_uses_legacy_v12_projection_and_ignores_wall_clock_timestamp() {
    let backend = load_test_backend_with_shared_loader(
        ActiveSystem::GameBoy,
        "rtc.gbc",
        build_gb_mbc3_rtc_test_rom(),
    );

    let mut canonicalized_raw = backend
        .encode_state_bytes()
        .expect("GB RTC backend should encode save-state");
    let replay_hash_state = backend
        .encode_replay_hash_state_bytes()
        .expect("GB RTC backend should encode replay-hash state");

    assert_ne!(
        replay_hash_state, canonicalized_raw,
        "raw GB RTC save-state should include a BESS wall-clock timestamp"
    );

    zeff_gb_core::save_state::project_replay_state_bytes(&mut canonicalized_raw).unwrap();
    zeff_gb_core::save_state::canonicalize_replay_hash_bytes(&mut canonicalized_raw);

    assert_eq!(
        replay_hash_state, canonicalized_raw,
        "replay-hash state should match the canonical legacy-v12 projection"
    );
    assert_eq!(
        u32::from_le_bytes(replay_hash_state[8..12].try_into().unwrap()),
        12
    );
}

#[test]
fn gb_replay_start_current_native_is_authoritative_and_legacy_v12_remains_loadable() {
    let mut backend = build_gb_backend();
    backend.step_frame();
    let mut seeded_state = backend.encode_state_bytes().unwrap();
    let extension_start = seeded_state
        .windows(4)
        .position(|window| window == b"ZBEX")
        .expect("current GB state should contain ZBEX");
    let authoritative_frame = backend.frame_count() + 123;
    seeded_state[extension_start + 8..extension_start + 16]
        .copy_from_slice(&authoritative_frame.to_le_bytes());
    for (index, byte) in seeded_state
        [extension_start + 16..extension_start + 16 + ActiveSystem::GameBoy.framebuffer_len()]
        .iter_mut()
        .enumerate()
    {
        *byte = 0x31_u8.wrapping_add(index as u8);
    }
    backend
        .load_state_from_bytes(seeded_state)
        .expect("seeded authoritative current state should load");
    let expected_framebuffer = backend.framebuffer().to_vec();
    assert!(expected_framebuffer.iter().any(|&byte| byte != 0));
    let replay_start = backend
        .encode_replay_start_state_bytes()
        .expect("GB backend should encode a current state");
    assert!(
        replay_start == backend.encode_state_bytes().unwrap(),
        "replay-start helper must preserve raw current-state bytes"
    );

    assert_eq!(
        u32::from_le_bytes(replay_start[8..12].try_into().unwrap()),
        zeff_gb_core::save_state::SAVE_STATE_FORMAT_VERSION
    );

    let probe = backend
        .probe_replay_state_load(&replay_start, None, false, false)
        .expect("current replay start should remain probeable");
    assert_eq!(probe.0, authoritative_frame);
    assert_eq!(probe.1, backend.game_boy_cpu_cycles());
    let mut restored = build_gb_backend();
    restored
        .load_state_from_bytes(replay_start.clone())
        .expect("current replay start should remain loadable");
    assert_eq!(restored.frame_count(), authoritative_frame);
    assert!(
        restored.framebuffer() == expected_framebuffer,
        "replay-start framebuffer differs"
    );
    assert_eq!(
        restored.game_boy_cpu_cycles(),
        backend.game_boy_cpu_cycles()
    );

    let mut legacy_start = replay_start;
    zeff_gb_core::save_state::project_replay_state_bytes(&mut legacy_start).unwrap();
    let legacy_probe = backend
        .probe_replay_state_load(&legacy_start, None, false, false)
        .expect("legacy v12 replay start should remain probeable");
    assert_eq!(legacy_probe.1, backend.game_boy_cpu_cycles());
    restored
        .load_state_from_bytes(legacy_start)
        .expect("legacy v12 replay start should remain loadable");
    assert_eq!(
        restored.game_boy_cpu_cycles(),
        backend.game_boy_cpu_cycles()
    );
}

#[test]
fn nes_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_nes_backend(), 1, 2);
}

#[test]
fn nes_replay_start_stays_v11_while_replay_hash_projects_to_v10() {
    let mut backend = build_nes_backend();
    backend.step_frame();
    let raw = backend.encode_state_bytes().unwrap();
    let replay_start = backend.encode_replay_start_state_bytes().unwrap();
    let replay_hash = backend.encode_replay_hash_state_bytes().unwrap();

    assert!(
        replay_start == raw,
        "NES replay start must preserve raw v11"
    );
    assert_eq!(
        u32::from_le_bytes(replay_start[8..12].try_into().unwrap()),
        11
    );
    assert_eq!(
        u32::from_le_bytes(replay_hash[8..12].try_into().unwrap()),
        10
    );
    let mut projected = raw.clone();
    zeff_nes_core::save_state::project_replay_state_bytes(&mut projected).unwrap();
    assert!(
        replay_hash == projected,
        "NES replay hash must use the legacy-v10 projection"
    );

    let expected_frame = backend.frame_count();
    let expected_framebuffer = backend.framebuffer().to_vec();
    let mut restored = build_nes_backend();
    restored.load_state_from_bytes(replay_start).unwrap();
    assert_eq!(restored.frame_count(), expected_frame);
    assert!(
        restored.framebuffer() == expected_framebuffer,
        "NES replay-start framebuffer differs"
    );
}

#[test]
fn pce_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_pce_backend(), 1, 2);
}

#[test]
fn gba_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_gba_backend(), 1, 2);
}

#[test]
fn ws_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_ws_backend(), 1, 2);
}

#[test]
fn sega8_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_sms_backend(), 1, 2);
}

#[test]
fn coleco_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_coleco_backend(), 1, 2);
}

#[test]
fn gb_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_gb_backend(),
        build_gb_backend(),
    );
}

#[test]
fn nes_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_nes_backend(),
        build_nes_backend(),
    );
}

#[test]
fn gba_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_gba_backend(),
        build_gba_backend(),
    );
}

#[test]
fn ws_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_ws_backend(),
        build_ws_backend(),
    );
}

#[test]
fn sega8_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_sms_backend(),
        build_sms_backend(),
    );
}

#[test]
fn coleco_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_coleco_backend(),
        build_coleco_backend(),
    );
}

#[test]
fn gba_backend_tracks_logical_rom_path_and_reload_source_path_separately() {
    let rom = build_gba_test_rom();
    let gba = zeff_gba_core::emulator::Emulator::new(&rom, 44_100)
        .expect("GBA emulator should initialize");
    let backend = EmuBackend::from_gba_with_source(
        gba,
        PathBuf::from("inside_archive.gba"),
        PathBuf::from("archive.zip"),
    );

    assert_eq!(backend.rom_path(), PathBuf::from("inside_archive.gba"));
    assert_eq!(backend.source_path(), PathBuf::from("archive.zip"));
}

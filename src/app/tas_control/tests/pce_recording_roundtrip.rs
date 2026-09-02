use super::harness::{app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame};
use super::*;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::{DirectPceCdTasExecutionLoader, DirectPceTasExecutionLoader};
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDigest, TasEditorSession,
    TasSeekStateCache,
};

#[test]
fn linked_app_records_pc_engine_input_and_commits_stop() {
    let root = crate::test_support::test_directory("tas-pce-live-record-roundtrip").unwrap();
    let rom_path = root.path().join("game.pce");
    std::fs::write(&rom_path, pce_rom()).unwrap();
    let loader = DirectPceTasExecutionLoader::new(rom_path.clone());
    let project = loader.create_project().unwrap();
    let expected_start = loader.load_editor_engine(&project).unwrap().into_backend();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 88, ActiveSystem::Pce, rom_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(88, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);

    for command in [
        LiveCommand::Button {
            player: 1,
            key: HostButton::Left,
            pressed: true,
        },
        LiveCommand::Button {
            player: 1,
            key: HostButton::A,
            pressed: true,
        },
    ] {
        live_ok(&mut app, command);
    }
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: true },
    );
    live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: false },
    );

    wait_for_recorded_frame(&mut app);
    let input = app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap()
        .selected_branch()
        .input_at(0);
    assert_eq!(
        input.players,
        [
            TasControllerInput {
                buttons: 0x01,
                dpad: 0x02,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ]
    );

    let mut expected = expected_start;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());
    let expected_state = TasDigest::from_bytes(&expected.encode_replay_hash_state_bytes().unwrap());
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 1,
            candidate_frame_count: 1,
            candidate_state_sha256,
            ..
        } if candidate_state_sha256 == expected_state
    ));
    assert!(!app.tas_control.realtime_recording_active());
}

fn linked_app_records_pc_engine_cd_memory_base_input_and_commits_stop(
    root: &std::path::Path,
    source_path: std::path::PathBuf,
    loader: DirectPceCdTasExecutionLoader,
) {
    let project = loader.create_project().unwrap();
    let expected_start = loader.load_editor_engine(&project).unwrap().into_backend();
    assert_eq!(
        expected_start.pce().unwrap().memory_base_mode(),
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled
    );
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 90, ActiveSystem::Pce, source_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(90, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);
    for command in [
        LiveCommand::Button {
            player: 1,
            key: HostButton::Left,
            pressed: true,
        },
        LiveCommand::Button {
            player: 1,
            key: HostButton::A,
            pressed: true,
        },
    ] {
        live_ok(&mut app, command);
    }
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: true },
    );
    live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: false },
    );

    wait_for_recorded_frame(&mut app);
    let input = app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap()
        .selected_branch()
        .input_at(0);
    assert_eq!(input.players[0].buttons, 0x01);
    assert_eq!(input.players[0].dpad, 0x02);
    let mut expected = expected_start;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());
    let expected_state = TasDigest::from_bytes(&expected.encode_replay_hash_state_bytes().unwrap());
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 1,
            candidate_frame_count: 1,
            candidate_state_sha256,
            ..
        } if candidate_state_sha256 == expected_state
    ));
    assert!(!app.tas_control.realtime_recording_active());
}

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}

#[test]
fn linked_app_records_pc_engine_cd_chd_memory_base_input_and_commits_stop() {
    let root = crate::test_support::test_directory("tas-pce-cd-chd-live-record-roundtrip").unwrap();
    let source_path = root.path().join("disc.chd");
    crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&source_path).unwrap();
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        zeff_firmware::sha256_bytes(system_card),
    );
    let normalized_disc_sha256 = loader
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            normalized_disc_sha256,
        );
    linked_app_records_pc_engine_cd_memory_base_input_and_commits_stop(
        root.path(),
        source_path,
        loader,
    );
}

#[test]
fn linked_app_records_pc_engine_cd_iso_memory_base_input_and_commits_stop() {
    let root = crate::test_support::test_directory("tas-pce-cd-iso-live-record-roundtrip").unwrap();
    let source_path = root.path().join("disc.iso");
    std::fs::write(&source_path, vec![0x5A; 4 * 2048]).unwrap();
    std::fs::write(
        root.path().join("disc.cue"),
        b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )
    .unwrap();
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        zeff_firmware::sha256_bytes(system_card),
    );
    let normalized_disc_sha256 = loader
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            normalized_disc_sha256,
        );
    linked_app_records_pc_engine_cd_memory_base_input_and_commits_stop(
        root.path(),
        source_path,
        loader,
    );
}

#[test]
fn linked_app_records_pc_engine_cd_ppf_memory_base_input_and_commits_stop() {
    let root = crate::test_support::test_directory("tas-pce-cd-ppf-live-record-roundtrip").unwrap();
    let source_path = root.path().join("disc.cue");
    std::fs::write(root.path().join("disc.bin"), vec![0x5A; 4 * 2048]).unwrap();
    std::fs::write(
        &source_path,
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )
    .unwrap();
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let system_card_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base_loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        system_card_sha256,
    );
    let source_disc_sha256 = base_loader
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            source_disc_sha256,
        );
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &source_path,
        vec![("memory-base.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )
    .unwrap();
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        source_path.clone(),
        system_card,
        system_card_sha256,
        stack,
    );
    linked_app_records_pc_engine_cd_memory_base_input_and_commits_stop(
        root.path(),
        source_path,
        loader,
    );
}

#[test]
fn linked_app_records_pc_engine_six_button_input_and_commits_stop() {
    let root =
        crate::test_support::test_directory("tas-pce-six-button-live-record-roundtrip").unwrap();
    let rom_path = root.path().join("game.pce");
    std::fs::write(&rom_path, pce_rom()).unwrap();
    let loader = DirectPceTasExecutionLoader::new_six_button(rom_path.clone());
    let project = loader.create_project().unwrap();
    let expected_start = loader.load_editor_engine(&project).unwrap().into_backend();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 90, ActiveSystem::Pce, rom_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(90, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);

    for command in [
        LiveCommand::Button {
            player: 1,
            key: HostButton::Left,
            pressed: true,
        },
        LiveCommand::Button {
            player: 1,
            key: HostButton::A,
            pressed: true,
        },
        LiveCommand::Button {
            player: 1,
            key: HostButton::L,
            pressed: true,
        },
        LiveCommand::Button {
            player: 1,
            key: HostButton::Y,
            pressed: true,
        },
    ] {
        live_ok(&mut app, command);
    }
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: true },
    );
    live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: false },
    );

    wait_for_recorded_frame(&mut app);
    let input = app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap()
        .selected_branch()
        .input_at(0);
    assert_eq!(
        input.players,
        [
            TasControllerInput {
                buttons: 0x91,
                dpad: 0x02,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ]
    );

    let mut expected = expected_start;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());
    let expected_state = TasDigest::from_bytes(&expected.encode_replay_hash_state_bytes().unwrap());
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 1,
            candidate_frame_count: 1,
            candidate_state_sha256,
            ..
        } if candidate_state_sha256 == expected_state
    ));
    assert!(!app.tas_control.realtime_recording_active());
}

#[test]
fn app_creates_direct_pce_project_and_rejects_wrong_media() {
    let root = crate::test_support::test_directory("tas-pce-app-project-creation").unwrap();
    let rom_path = root.path().join("game.pce");
    std::fs::write(&rom_path, pce_rom()).unwrap();
    let loader = DirectPceTasExecutionLoader::new(rom_path.clone());
    let initial_project = loader.create_project().unwrap();
    let backend = loader
        .load_editor_engine(&initial_project)
        .unwrap()
        .into_backend();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 89, ActiveSystem::Pce, rom_path);
    let project_path = root.path().join("movie.ztas");

    app.create_tas_project_for_live_control(project_path.clone(), false)
        .unwrap();
    let session = app.debug_windows.tas_editor.active_session().unwrap();
    assert_eq!(session.project().identity().system, "pce");
    assert_eq!(
        session.project().identity().devices[0].device,
        "pce-two-button-controller"
    );
    assert!(project_path.exists());

    app.settings.emulation.pce_controller = crate::settings::PceControllerPreference::SixButton;
    let six_button_project_path = root.path().join("six-button.ztas");
    app.create_tas_project_for_live_control(six_button_project_path.clone(), false)
        .unwrap();
    assert_eq!(
        app.debug_windows
            .tas_editor
            .active_session()
            .unwrap()
            .project()
            .identity()
            .devices[0]
            .device,
        "pce-six-button-controller"
    );
    assert!(six_button_project_path.exists());

    let wrong_path = root.path().join("wrong.bin");
    std::fs::write(&wrong_path, pce_rom()).unwrap();
    app.rom_info.source_path = Some(wrong_path);
    let rejected_path = root.path().join("wrong.ztas");
    assert!(
        app.create_tas_project_for_live_control(rejected_path.clone(), false)
            .is_err()
    );
    assert!(!rejected_path.exists());
}

fn pce_rom() -> Vec<u8> {
    let mut rom = vec![0; zeff_pce_core::hardware::PCEAS_HEADER_LEN];
    rom[0] = 1;
    rom.extend(vec![0xEA; 0x2000]);
    rom
}

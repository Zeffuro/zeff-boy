use super::harness::{app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame};
use super::*;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::DirectGbaTasExecutionLoader;
use crate::emu_backend::{BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDigest, TasEditorSession,
    TasSeekStateCache,
};

#[test]
fn linked_app_records_one_gba_keypad_and_commits_an_in_flight_stop() {
    let root = crate::test_support::test_directory("tas-gba-live-record-roundtrip").unwrap();
    let rom_path = root.path().join("game.gba");
    std::fs::write(&rom_path, gba_rom()).unwrap();
    let loader = DirectGbaTasExecutionLoader::new(rom_path.clone());
    let project = loader.create_project().unwrap();
    let expected_start = loader.load_editor_engine(&project).unwrap().into_backend();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 89, ActiveSystem::GameBoyAdvance, rom_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(89, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);

    for command in [
        LiveCommand::Button {
            player: 1,
            key: HostButton::Right,
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
    ] {
        live_ok(&mut app, command);
    }
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: true },
    );
    let started = live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    assert_eq!(started["live"]["state"], "recording");
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
                buttons: 0x11,
                dpad: 0x01,
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
fn app_creates_direct_gba_projects_and_rejects_non_gba_media() {
    let root = crate::test_support::test_directory("tas-gba-app-project-creation").unwrap();
    let rom_path = root.path().join("game.gba");
    std::fs::write(&rom_path, gba_rom()).unwrap();
    let loader = DirectGbaTasExecutionLoader::new(rom_path.clone());
    let initial_project = loader.create_project().unwrap();
    let backend = loader
        .load_editor_engine(&initial_project)
        .unwrap()
        .into_backend();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 90, ActiveSystem::GameBoyAdvance, rom_path);
    let project_path = root.path().join("movie.ztas");

    app.create_tas_project_for_live_control(project_path.clone(), false)
        .unwrap();
    let session = app.debug_windows.tas_editor.active_session().unwrap();
    assert_eq!(session.project().identity().system, "gba");
    assert_eq!(session.project().identity().devices.len(), 1);
    assert!(project_path.exists());

    let wrong_path = root.path().join("game.gb");
    std::fs::write(&wrong_path, gba_rom()).unwrap();
    app.active_system = ActiveSystem::GameBoyAdvance;
    app.rom_info.source_path = Some(wrong_path);
    let rejected_path = root.path().join("wrong.ztas");
    assert!(
        app.create_tas_project_for_live_control(rejected_path.clone(), false)
            .is_err()
    );
    assert!(!rejected_path.exists());
}

#[test]
fn new_battery_project_reloads_and_connects_automatically() {
    let root = crate::test_support::test_directory("tas-gba-battery-project-connect").unwrap();
    let archive_path = root.path().join("game.zip");
    let rom_path = archive_path.join("games/game.gba");
    let mut rom = gba_rom();
    rom.extend_from_slice(b"SRAM_V113");
    let save = vec![0xA5; zeff_gba_core::hardware::constants::SRAM_SIZE];
    crate::test_support::write_zip(&archive_path, &[("games/game.gba", &rom)]).unwrap();
    std::fs::write(archive_path.with_extension("sav"), save).unwrap();
    let selected = crate::rom_archive::extract_bounded_zip_member(
        &archive_path,
        Some(&rom_path),
        "gba",
        128 * 1024 * 1024,
        32 * 1024 * 1024,
    )
    .unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::GameBoyAdvance,
        &archive_path,
        &selected.rom_path,
        Some(selected.bytes),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            apply_mods: false,
            initial_input: Some((0, 0)),
            gba_load_battery_sram: true,
            gba_seed_rtc_from_host: false,
            gba_use_external_bios: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(
        worker,
        91,
        ActiveSystem::GameBoyAdvance,
        archive_path.clone(),
    );
    app.rom_info.rom_path = Some(rom_path);

    app.frames_in_flight = 1;
    app.create_tas_project_for_live_control(root.path().join("movie.ztas"), false)
        .unwrap();
    assert_ne!(
        app.debug_windows
            .tas_editor
            .active_session()
            .unwrap()
            .project()
            .identity()
            .persistent_state,
        crate::tas_project::TasExternalIdentity::Absent
    );
    assert!(app.pending_tas_repair_activation.is_some());
    assert!(!app.worker_gameplay_commands_allowed());
    assert!(matches!(
        app.tas_repair_state(),
        crate::app::tas_control::repair::TasRepairState::Detached
    ));
    app.frames_in_flight = 0;
    app.pump_pending_tas_repair_activation();
    assert!(matches!(
        app.tas_repair_state(),
        crate::app::tas_control::repair::TasRepairState::RepairedDetached { .. }
    ));
    wait_for_linked(&mut app);
}

fn gba_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom
}

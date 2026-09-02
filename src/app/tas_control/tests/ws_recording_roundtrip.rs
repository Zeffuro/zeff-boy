use super::harness::{app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame};
use super::*;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::DirectWsTasExecutionLoader;
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDigest, TasEditorSession,
    TasSeekStateCache,
};

#[test]
fn linked_app_records_vertical_wonderswan_p1_input_and_commits_stop() {
    let root = crate::test_support::test_directory("tas-ws-live-record-roundtrip").unwrap();
    let rom_path = root.path().join("vertical.wsc");
    std::fs::write(&rom_path, ws_rom(true, true)).unwrap();
    let loader = DirectWsTasExecutionLoader::new(rom_path.clone());
    let project = loader.create_project().unwrap();
    let expected_start = loader.load_editor_engine(&project).unwrap().into_backend();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 86, ActiveSystem::WonderSwan, rom_path);
    app.ws_display_rotated = true;
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(86, snapshot, TasControlStartMode::Preview)
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
                buttons: 0x81,
                dpad: 0,
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
fn app_creates_direct_wonderswan_projects_and_rejects_mismatched_media() {
    let root = crate::test_support::test_directory("tas-ws-app-project-creation").unwrap();
    let rom_path = root.path().join("game.ws");
    std::fs::write(&rom_path, ws_rom(false, false)).unwrap();
    let loader = DirectWsTasExecutionLoader::new(rom_path.clone());
    let initial_project = loader.create_project().unwrap();
    let backend = loader
        .load_editor_engine(&initial_project)
        .unwrap()
        .into_backend();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 87, ActiveSystem::WonderSwan, rom_path);
    let project_path = root.path().join("movie.ztas");

    app.create_tas_project_for_live_control(project_path.clone(), false)
        .unwrap();
    let session = app.debug_windows.tas_editor.active_session().unwrap();
    assert_eq!(session.project().identity().system, "ws");
    assert_eq!(
        session.project().identity().devices[0].device,
        "ws-standard-keypad-horizontal"
    );
    assert!(project_path.exists());

    let color_path = root.path().join("vertical.wsc");
    std::fs::write(&color_path, ws_rom(true, true)).unwrap();
    app.active_system = ActiveSystem::WonderSwan;
    app.rom_info.source_path = Some(color_path);
    let color_project_path = root.path().join("vertical.ztas");
    app.create_tas_project_for_live_control(color_project_path.clone(), false)
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
        "ws-standard-keypad-vertical"
    );
    assert!(color_project_path.exists());

    let mismatched_path = root.path().join("mono.wsc");
    std::fs::write(&mismatched_path, ws_rom(false, false)).unwrap();
    app.active_system = ActiveSystem::WonderSwan;
    app.rom_info.source_path = Some(mismatched_path);
    let rejected_path = root.path().join("mismatched.ztas");
    assert!(
        app.create_tas_project_for_live_control(rejected_path.clone(), false)
            .is_err()
    );
    assert!(!rejected_path.exists());

    let other_path = root.path().join("other.gb");
    std::fs::write(&other_path, ws_rom(false, false)).unwrap();
    app.rom_info.source_path = Some(other_path);
    let rejected_path = root.path().join("other.ztas");
    assert!(
        app.create_tas_project_for_live_control(rejected_path.clone(), false)
            .is_err()
    );
    assert!(!rejected_path.exists());
}

fn ws_rom(color: bool, vertical: bool) -> Vec<u8> {
    let mut rom = vec![0x90; 128 * 1024];
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer + 1] = u8::from(color);
    rom[footer + 4] = 0x01;
    rom[footer + 5] = 0;
    rom[footer + 6] = u8::from(vertical);
    rom[footer + 7] = 0;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

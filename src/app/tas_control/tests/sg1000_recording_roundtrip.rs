use super::harness::{app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame};
use super::*;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::DirectSg1000TasExecutionLoader;
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDigest, TasEditorSession,
    TasSeekStateCache,
};

#[test]
fn linked_app_records_two_sg1000_pads_and_commits_an_in_flight_stop() {
    let root = crate::test_support::test_directory("tas-sg1000-live-record-roundtrip").unwrap();
    let rom_path = root.path().join("game.sg");
    std::fs::write(&rom_path, sg1000_rom()).unwrap();
    let loader = DirectSg1000TasExecutionLoader::new(rom_path.clone());
    let project = loader.create_project().unwrap();
    let expected_start = loader.load_editor_engine(&project).unwrap().into_backend();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 83, ActiveSystem::Sg1000, rom_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(83, snapshot, TasControlStartMode::Preview)
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
            player: 2,
            key: HostButton::Right,
            pressed: true,
        },
        LiveCommand::Button {
            player: 2,
            key: HostButton::B,
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
            TasControllerInput {
                buttons: 0x02,
                dpad: 0x01,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ]
    );

    let mut expected = expected_start;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.set_input_p2(input.players[1].buttons, input.players[1].dpad);
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
fn app_creates_direct_sg1000_project_and_rejects_other_sega_media() {
    let root = crate::test_support::test_directory("tas-sg1000-app-project-creation").unwrap();
    let rom_path = root.path().join("game.sc");
    std::fs::write(&rom_path, sg1000_rom()).unwrap();
    let loader = DirectSg1000TasExecutionLoader::new(rom_path.clone());
    let initial_project = loader.create_project().unwrap();
    let backend = loader
        .load_editor_engine(&initial_project)
        .unwrap()
        .into_backend();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 84, ActiveSystem::Sg1000, rom_path);
    let project_path = root.path().join("movie.ztas");

    app.create_tas_project_for_live_control(project_path.clone(), false)
        .unwrap();
    let session = app.debug_windows.tas_editor.active_session().unwrap();
    assert_eq!(session.project().identity().system, "sg");
    assert_eq!(session.project().identity().devices.len(), 2);
    assert!(project_path.exists());

    let sms_path = root.path().join("other.sms");
    std::fs::write(&sms_path, sg1000_rom()).unwrap();
    app.active_system = ActiveSystem::Sg1000;
    app.rom_info.source_path = Some(sms_path);
    let rejected_path = root.path().join("other-sega.ztas");
    assert!(
        app.create_tas_project_for_live_control(rejected_path.clone(), false)
            .is_err()
    );
    assert!(!rejected_path.exists());

    let game_gear_path = root.path().join("game.gg");
    std::fs::write(&game_gear_path, sg1000_rom()).unwrap();
    app.active_system = ActiveSystem::GameGear;
    app.rom_info.source_path = Some(game_gear_path);
    let rejected_path = root.path().join("game-gear.ztas");
    assert!(
        app.create_tas_project_for_live_control(rejected_path.clone(), false)
            .is_err()
    );
    assert!(!rejected_path.exists());
}

fn sg1000_rom() -> Vec<u8> {
    let mut rom = vec![0; 8 * 1024];
    rom[..2].copy_from_slice(&[0x18, 0xFE]);
    rom
}

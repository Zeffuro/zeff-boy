use super::harness::{app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame};
use super::*;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::DirectColecoTasExecutionLoader;
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasColecoControllerInput, TasColecoKeypadKey, TasDigest,
    TasEditorSession, TasSeekStateCache,
};

static TEST_BIOS: [u8; zeff_coleco_core::constants::BIOS_SIZE] =
    [0; zeff_coleco_core::constants::BIOS_SIZE];

#[test]
fn linked_app_records_two_coleco_controllers_and_commits_an_in_flight_stop() {
    let root = crate::test_support::test_directory("tas-coleco-live-record-roundtrip").unwrap();
    let rom_path = root.path().join("game.col");
    let mut rom = vec![0; 8 * 1024];
    rom[..2].copy_from_slice(&[0xAA, 0x55]);
    std::fs::write(&rom_path, rom).unwrap();
    let loader = DirectColecoTasExecutionLoader::new_with_bios_override(
        rom_path.clone(),
        Vec::new(),
        &TEST_BIOS,
    );
    let project = loader.create_project().unwrap();
    let expected_start = loader.load_editor_engine(&project).unwrap().into_backend();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 72, ActiveSystem::Coleco, rom_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(72, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);

    live_ok(
        &mut app,
        LiveCommand::Button {
            player: 1,
            key: HostButton::Left,
            pressed: true,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::Button {
            player: 1,
            key: HostButton::A,
            pressed: true,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::ColecoKeypad {
            player: 1,
            key: 10,
            pressed: true,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::Button {
            player: 2,
            key: HostButton::Right,
            pressed: true,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::Button {
            player: 2,
            key: HostButton::B,
            pressed: true,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::ColecoKeypad {
            player: 2,
            key: 9,
            pressed: true,
        },
    );

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
        input.coleco,
        [
            TasColecoControllerInput {
                left: true,
                left_button: true,
                keypad: TasColecoKeypadKey::Star,
                ..TasColecoControllerInput::default()
            },
            TasColecoControllerInput {
                right: true,
                right_button: true,
                keypad: TasColecoKeypadKey::Nine,
                ..TasColecoControllerInput::default()
            },
        ]
    );
    assert_eq!(
        input.players,
        [crate::tas_project::TasControllerInput::default(); 5]
    );

    let mut expected = expected_start;
    expected.apply_coleco_tas_input(input.coleco).unwrap();
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

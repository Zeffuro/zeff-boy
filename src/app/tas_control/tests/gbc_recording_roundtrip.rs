use std::time::{Duration, Instant};

use super::harness::{app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame};
use super::*;
use crate::app::App;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::DirectGbcTasExecutionLoader;
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::{TasControllerInput, TasDigest};

#[test]
fn direct_gbc_app_creates_links_records_and_disconnects() {
    let root = crate::test_support::test_directory("tas-gbc-live-record-roundtrip").unwrap();
    let rom_path = root.path().join("game.gbc");
    let mut rom = crate::test_support::build_gb_test_rom();
    rom[0x143] = 0xC0;
    std::fs::write(&rom_path, rom).unwrap();
    let loader = DirectGbcTasExecutionLoader::new(rom_path.clone(), Vec::new());
    let project = loader.create_project().unwrap();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 91, ActiveSystem::GameBoy, rom_path);
    let project_path = root.path().join("movie.ztas");

    app.create_tas_project_for_live_control(project_path.clone(), false)
        .unwrap();
    assert_eq!(
        app.debug_windows
            .tas_editor
            .active_session()
            .unwrap()
            .project()
            .identity()
            .system,
        "gb"
    );
    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    assert_eq!(snapshot.profile, TasExecutionProfile::DirectGbCartridgeCgb);
    app.tas_control
        .queue_acquire(91, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);

    live_ok(
        &mut app,
        LiveCommand::Button {
            player: 1,
            key: HostButton::Right,
            pressed: true,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::Button {
            player: 1,
            key: HostButton::B,
            pressed: true,
        },
    );
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
        input.players[0],
        TasControllerInput {
            buttons: 0x02,
            dpad: 0x01,
        }
    );
    expected.apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
        buttons: input.players[0].buttons,
        dpad: input.players[0].dpad,
        ..Default::default()
    });
    expected.step_frame();
    let expected_state = TasDigest::from_bytes(&expected.encode_state_bytes().unwrap());
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 1,
            candidate_state_sha256,
            ..
        } if candidate_state_sha256 == expected_state
    ));

    let reply = live_ok(&mut app, LiveCommand::TasDisconnect { keep: true });
    assert_eq!(reply["live"]["state"], "keeping");
    wait_for_detached(&mut app);
    assert!(project_path.exists());
}

fn wait_for_detached(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.tas_control.state != TasControlState::Detached && Instant::now() < deadline {
        app.drain_emu_responses();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.tas_control.state, TasControlState::Detached);
}

use std::time::{Duration, Instant};

use serde_json::Value;

use super::harness::{app_with_worker, live_ok};
use super::*;
use crate::app::App;
use crate::emu_backend::loader::DirectNesTasExecutionLoader;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::LiveCommand;
use crate::tas_project::{TasControllerInput, TasInputFrame};

#[test]
fn live_command_playback_reaches_end_without_editing_or_consuming_end() {
    let root = crate::test_support::test_directory("tas-playback-live-roundtrip").unwrap();
    let rom_path = root.path().join("playback.nes");
    std::fs::write(&rom_path, crate::test_support::build_nes_test_rom()).unwrap();
    let loader = DirectNesTasExecutionLoader::new(rom_path.clone(), Vec::new());
    let mut project = loader.create_project().unwrap();
    project
        .edit_transaction(|edit| {
            edit.insert_frames("main", 1, 2)?;
            edit.set_input_range(
                "main",
                1,
                1,
                TasInputFrame {
                    players: [
                        TasControllerInput {
                            buttons: 0x02,
                            dpad: 0x01,
                        },
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                    ],
                    ..TasInputFrame::default()
                },
            )
        })
        .unwrap();
    let project_path = root.path().join("playback.ztas");
    project.save_atomic(&project_path).unwrap();

    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig {
            apply_mods: false,
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 70, ActiveSystem::Nes, rom_path);

    live_ok(&mut app, LiveCommand::TasOpenProject { path: project_path });
    wait_for(&mut app, |status| status["readiness"]["state"] == "ready");
    live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 1 });
    live_ok(
        &mut app,
        LiveCommand::TasLink {
            at_end: false,
            record: false,
        },
    );
    wait_for(&mut app, |status| {
        status["live"]["state"] == "linked" && status["live"]["execution_boundary"] == 1
    });
    let session = app.debug_windows.tas_editor.active_session().unwrap();
    let before = session.project().encode().unwrap();
    let rerecords = session.project().rerecord_count();
    app.remote_debug_frames_remaining = 1;

    live_ok(
        &mut app,
        LiveCommand::Button {
            player: 1,
            key: HostButton::A,
            pressed: true,
        },
    );
    let started = live_ok(&mut app, LiveCommand::TasSetPlayback { active: true });
    assert_eq!(started["live"]["state"], "playing");
    let ended = wait_for(&mut app, |status| {
        status["live"]["state"] == "linked" && status["live"]["execution_boundary"] == 3
    });
    assert_eq!(ended["project"]["frame_count"], 3);

    let session = app.debug_windows.tas_editor.active_session().unwrap();
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.project().rerecord_count(), rerecords);
    assert!(
        app.cached_ui_data
            .as_ref()
            .and_then(|data| data.cpu_debug.as_ref())
            .is_some()
    );
    let settled_frame_count = match &app.tas_control.state {
        TasControlState::AwaitingDecision {
            candidate_frame_count,
            next_advance_id,
            ..
        } => {
            assert_eq!(*next_advance_id, 3);
            *candidate_frame_count
        }
        state => panic!("expected settled linked state, got {state:?}"),
    };
    std::thread::sleep(Duration::from_millis(25));
    pump(&mut app);
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_frame_count,
            next_advance_id: 3,
            candidate_executed_project_frames: 3,
            ..
        } if candidate_frame_count == settled_frame_count
    ));
}

#[test]
fn closing_during_an_in_flight_playback_frame_keeps_the_settled_position() {
    let root = crate::test_support::test_directory("tas-playback-close-in-flight").unwrap();
    let rom_path = root.path().join("playback-close.nes");
    std::fs::write(&rom_path, crate::test_support::build_nes_test_rom()).unwrap();
    let loader = DirectNesTasExecutionLoader::new(rom_path.clone(), Vec::new());
    let mut project = loader.create_project().unwrap();
    project
        .edit_transaction(|edit| edit.insert_frames("main", 1, 2))
        .unwrap();
    let project_path = root.path().join("playback-close.ztas");
    project.save_atomic(&project_path).unwrap();

    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig {
            apply_mods: false,
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 71, ActiveSystem::Nes, rom_path);

    live_ok(&mut app, LiveCommand::TasOpenProject { path: project_path });
    wait_for(&mut app, |status| status["readiness"]["state"] == "ready");
    live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 1 });
    live_ok(
        &mut app,
        LiveCommand::TasLink {
            at_end: false,
            record: false,
        },
    );
    wait_for(&mut app, |status| {
        status["live"]["state"] == "linked" && status["live"]["execution_boundary"] == 1
    });
    let session = app.debug_windows.tas_editor.active_session().unwrap();
    let before = session.project().encode().unwrap();

    live_ok(&mut app, LiveCommand::TasSetPlayback { active: true });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        app.tas_control.state,
        TasControlState::PlaybackPending { .. }
    ) && Instant::now() < deadline
    {
        pump(&mut app);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        app.tas_control.state,
        TasControlState::PlaybackPending {
            expected_executed_project_frames: 2,
            ..
        }
    ));

    app.refresh_tas_editor_live_status();
    app.debug_windows.tas_editor.close();
    let request = app
        .debug_windows
        .tas_editor
        .take_pending_host_request()
        .expect("closing an active playback should first request a pause");
    app.handle_tas_editor_host_request(request);

    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 2,
            ..
        }
    ) && Instant::now() < deadline
    {
        pump(&mut app);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 2,
            ..
        }
    ));

    app.refresh_tas_editor_live_status();
    wait_for(&mut app, |status| status["live"]["state"] == "ready");
    assert_eq!(
        app.debug_windows
            .tas_editor
            .active_session()
            .unwrap()
            .project()
            .encode()
            .unwrap(),
        before
    );
}

fn pump(app: &mut App) {
    app.drain_emu_responses();
    app.begin_queued_tas_control_acquire();
    app.pump_linked_tas_playback();
}

fn wait_for(app: &mut App, ready: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = live_ok(app, LiveCommand::TasStatus);
    while !ready(&last) && Instant::now() < deadline {
        pump(app);
        last = live_ok(app, LiveCommand::TasStatus);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(ready(&last), "timed out waiting for TAS playback: {last}");
    last
}

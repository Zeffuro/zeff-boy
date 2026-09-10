use std::time::{Duration, Instant};

use super::harness::{app_with_worker, live_ok};
use super::*;
use crate::app::App;
use crate::emu_backend::loader::DirectNesTasExecutionLoader;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::TasDigest;
use winit::event::WindowEvent;

fn open_app(root: &std::path::Path) -> App {
    let rom_path = root.join("window.nes");
    std::fs::write(&rom_path, crate::test_support::build_nes_test_rom()).unwrap();
    let mut project = DirectNesTasExecutionLoader::new(rom_path.clone(), Vec::new())
        .create_project()
        .unwrap();
    project
        .edit_transaction(|edit| edit.insert_frames("main", 1, 3))
        .unwrap();
    let project_path = root.join("window.ztas");
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
    let mut app = app_with_worker(
        EmuThread::spawn(backend, false),
        172,
        ActiveSystem::Nes,
        rom_path,
    );
    live_ok(&mut app, LiveCommand::TasOpenProject { path: project_path });
    settle(&mut app, |app| {
        app.tas_control_readiness_report().is_some_and(|report| {
            report.status == crate::app::tas_control::readiness::TasReadinessStatus::Ready
        })
    });
    app
}

fn settle(app: &mut App, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready(app) && Instant::now() < deadline {
        app.begin_queued_tas_control_acquire();
        app.drain_emu_responses();
        app.refresh_tas_editor_live_status();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        ready(app),
        "TAS window lifecycle timed out: {:?}",
        app.tas_control.state
    );
}

fn capture_state(app: &App) -> Vec<u8> {
    let worker = app.emu_thread.as_ref().unwrap();
    assert!(worker.send_checked(EmuCommand::CaptureStateBytes));
    match worker.recv_checked().unwrap() {
        EmuResponse::StateCaptured(bytes) => bytes,
        _ => panic!("unexpected capture response"),
    }
}

fn linked_at(app: &App, cursor: u64) -> bool {
    matches!(app.tas_control.state, TasControlState::AwaitingDecision { candidate_executed_project_frames, .. } if candidate_executed_project_frames == cursor)
}

fn begin_live_recording_frame(app: &mut App) {
    live_ok(
        app,
        LiveCommand::TasLink {
            at_end: true,
            record: false,
        },
    );
    settle(app, |app| linked_at(app, 4));
    live_ok(app, LiveCommand::TasSetRealtimeRecording { active: true });
    live_ok(
        app,
        LiveCommand::Button {
            player: 1,
            key: HostButton::A,
            pressed: true,
        },
    );
    live_ok(
        app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    assert!(matches!(
        app.tas_control.state,
        TasControlState::FrameAdvancePending {
            expected_executed_project_frames: 5,
            ..
        }
    ));
    assert!(app.tas_control.realtime_recording_active());
    assert_recording_project(app, 4);
}

fn receive_live_frame(app: &App) -> (EmuResponse, TasDigest) {
    let response = app.emu_thread.as_ref().unwrap().recv_checked().unwrap();
    let EmuResponse::TasFrameAdvanced {
        executed_project_frames: 5,
        state_sha256,
        ..
    } = &response
    else {
        panic!("unexpected live-frame response");
    };
    let state_sha256 = *state_sha256;
    (response, state_sha256)
}

fn assert_recording_project(app: &App, expected_frames: u64) {
    let session = app.debug_windows.tas_editor.active_session().unwrap();
    assert_eq!(session.selected_branch().frame_count(), expected_frames);
    assert_eq!(session.cursor(), expected_frames);
    if expected_frames == 5 {
        let recorded = session.selected_branch().input_at(4);
        assert_eq!(recorded.players[0].buttons, 0x01);
        assert_eq!(recorded.players[0].dpad, 0);
        assert!(
            recorded.players[1..]
                .iter()
                .all(|input| *input == Default::default())
        );
        assert_eq!(recorded.zapper, Default::default());
    }
}

fn finish_closed_recording(app: &mut App, accepted_state: TasDigest) {
    settle(app, |app| {
        app.tas_control.state == TasControlState::Detached && !app.tas_control.readiness_pending()
    });
    assert!(!app.debug_windows.tas_editor.open);
    assert_eq!(TasDigest::from_bytes(&capture_state(app)), accepted_state);
    assert_recording_project(app, 5);

    let accepted_project = app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap()
        .project()
        .encode()
        .unwrap();
    app.handle_tas_editor_window_event(WindowEvent::CloseRequested);
    assert_eq!(TasDigest::from_bytes(&capture_state(app)), accepted_state);
    assert_eq!(
        app.debug_windows
            .tas_editor
            .active_session()
            .unwrap()
            .project()
            .encode()
            .unwrap(),
        accepted_project
    );

    app.debug_windows.tas_editor.open_separate_window();
    settle(app, |app| {
        app.tas_control.state == TasControlState::Detached && !app.tas_control.readiness_pending()
    });
    assert!(app.debug_windows.tas_editor.open);
    assert_recording_project(app, 5);
    app.handle_tas_editor_window_event(WindowEvent::CloseRequested);
    assert_eq!(app.tas_control.state, TasControlState::Detached);
    assert_eq!(TasDigest::from_bytes(&capture_state(app)), accepted_state);
    assert_eq!(
        app.debug_windows
            .tas_editor
            .active_session()
            .unwrap()
            .project()
            .encode()
            .unwrap(),
        accepted_project
    );
}

#[test]
fn native_close_during_initial_acquisition_restores_exact_original_state() {
    close_initial_and_check_restore(false);
}

#[test]
fn native_close_during_initial_execution_restores_exact_original_state() {
    close_initial_and_check_restore(true);
}

fn close_initial_and_check_restore(execution_started: bool) {
    let root = crate::test_support::test_directory(if execution_started {
        "tas-window-close-execution"
    } else {
        "tas-window-close-acquisition"
    })
    .unwrap();
    let mut app = open_app(root.path());
    let original = capture_state(&app);
    live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 3 });
    live_ok(
        &mut app,
        LiveCommand::TasLink {
            at_end: false,
            record: false,
        },
    );
    app.begin_queued_tas_control_acquire();
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AcquirePending { .. }
    ));
    if execution_started {
        let acquired = app.emu_thread.as_ref().unwrap().recv_checked().unwrap();
        assert!(matches!(acquired, EmuResponse::TasControlAcquired { .. }));
        assert!(app.consume_tas_control_response(acquired).is_none());
        assert!(matches!(
            app.tas_control.state,
            TasControlState::ExecutionPending { .. }
        ));
        app.refresh_tas_editor_live_status();
    }
    assert!(!app.tas_control.gameplay_commands_allowed());
    app.handle_tas_editor_window_event(WindowEvent::CloseRequested);
    settle(&mut app, |app| {
        app.tas_control.state == TasControlState::Detached && !app.tas_control.readiness_pending()
    });
    assert!(!app.debug_windows.tas_editor.open);
    assert_eq!(capture_state(&app), original);
    app.handle_tas_editor_window_event(WindowEvent::CloseRequested);
    assert_eq!(capture_state(&app), original);
}

#[test]
fn native_close_during_linked_seek_keeps_exact_completed_state_once() {
    let root = crate::test_support::test_directory("tas-window-close-seek").unwrap();
    let mut app = open_app(root.path());
    live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 3 });
    live_ok(
        &mut app,
        LiveCommand::TasLink {
            at_end: false,
            record: false,
        },
    );
    settle(&mut app, |app| linked_at(app, 3));
    live_ok(&mut app, LiveCommand::TasDisconnect { keep: true });
    settle(&mut app, |app| {
        app.tas_control.state == TasControlState::Detached && !app.tas_control.readiness_pending()
    });
    let expected = capture_state(&app);
    let project_bytes = app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap()
        .project()
        .encode()
        .unwrap();
    live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 1 });
    live_ok(
        &mut app,
        LiveCommand::TasLink {
            at_end: false,
            record: false,
        },
    );
    settle(&mut app, |app| linked_at(app, 1));
    live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 3 });
    app.seek_linked_tas_to_editor_cursor().unwrap();
    app.refresh_tas_editor_live_status();
    assert!(matches!(
        app.tas_control.state,
        TasControlState::ExecutionPending { .. }
    ));
    app.handle_tas_editor_window_event(WindowEvent::CloseRequested);
    settle(&mut app, |app| {
        app.tas_control.state == TasControlState::Detached && !app.tas_control.readiness_pending()
    });
    assert!(!app.debug_windows.tas_editor.open);
    assert_eq!(capture_state(&app), expected);
    assert_eq!(
        app.debug_windows
            .tas_editor
            .active_session()
            .unwrap()
            .project()
            .encode()
            .unwrap(),
        project_bytes
    );
    app.handle_tas_editor_window_event(WindowEvent::CloseRequested);
    assert_eq!(capture_state(&app), expected);
}

#[test]
fn native_close_before_live_record_response_keeps_exact_accepted_frame_once() {
    let root = crate::test_support::test_directory("tas-window-close-recording-response").unwrap();
    let mut app = open_app(root.path());
    begin_live_recording_frame(&mut app);

    app.handle_tas_editor_window_event(WindowEvent::CloseRequested);
    assert!(!app.debug_windows.tas_editor.open);
    assert!(!app.tas_control.realtime_recording_active());
    assert!(matches!(
        app.tas_control.state,
        TasControlState::FrameAdvancePending { .. }
    ));
    assert_recording_project(&app, 4);

    let (response, accepted_state) = receive_live_frame(&app);
    assert!(app.consume_tas_control_response(response).is_none());
    app.refresh_tas_editor_live_status();
    finish_closed_recording(&mut app, accepted_state);
}

#[test]
fn native_close_during_live_frame_project_commit_keeps_exact_accepted_frame_once() {
    let root = crate::test_support::test_directory("tas-window-close-recording-commit").unwrap();
    let mut app = open_app(root.path());
    begin_live_recording_frame(&mut app);
    let current =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    let (response, accepted_state) = receive_live_frame(&app);
    let ResponseDisposition::CommitLiveFrame { prepared, .. } =
        app.tas_control
            .consume_response(app.emu_worker_generation, response, None, Some(&current))
    else {
        panic!("matching worker response should await the editor commit");
    };
    assert!(matches!(
        app.tas_control.state,
        TasControlState::FrameRecordCommitPending { .. }
    ));
    assert_recording_project(&app, 4);

    app.handle_tas_editor_window_event(WindowEvent::CloseRequested);
    assert!(!app.debug_windows.tas_editor.open);
    assert!(!app.tas_control.realtime_recording_active());
    assert!(matches!(
        app.tas_control.state,
        TasControlState::FrameRecordCommitPending { .. }
    ));
    app.debug_windows
        .tas_editor
        .commit_prepared_live_frame(*prepared)
        .unwrap();
    let committed =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap());
    assert!(matches!(
        app.tas_control.finish_live_frame_commit(committed),
        ResponseDisposition::Consumed { follow_up: None }
    ));
    assert_recording_project(&app, 5);
    app.refresh_tas_editor_live_status();
    finish_closed_recording(&mut app, accepted_state);
}

#[test]
fn native_close_after_live_frame_commit_keeps_exact_accepted_frame_once() {
    let root = crate::test_support::test_directory("tas-window-close-recording-accepted").unwrap();
    let mut app = open_app(root.path());
    begin_live_recording_frame(&mut app);
    let (response, accepted_state) = receive_live_frame(&app);
    assert!(app.consume_tas_control_response(response).is_none());
    assert!(linked_at(&app, 5));
    assert!(app.tas_control.realtime_recording_active());
    assert_recording_project(&app, 5);

    app.handle_tas_editor_window_event(WindowEvent::CloseRequested);
    finish_closed_recording(&mut app, accepted_state);
}

#[test]
fn native_recording_game_to_editor_focus_accepts_pending_then_resumes_without_stuck_input() {
    recording_focus_transfer(true);
}

#[test]
fn native_recording_app_focus_loss_accepts_pending_then_resumes_without_stuck_input() {
    recording_focus_transfer(false);
}

fn recording_focus_transfer(editor_focused: bool) {
    let root = crate::test_support::test_directory(if editor_focused {
        "tas-recording-editor-focus"
    } else {
        "tas-recording-app-focus"
    })
    .unwrap();
    let mut app = open_app(root.path());
    app.settings.emulation.pause_on_unfocus = false;
    app.debug_windows.tas_editor.open_separate_window();
    live_ok(
        &mut app,
        LiveCommand::TasLink {
            at_end: true,
            record: false,
        },
    );
    settle(&mut app, |app| linked_at(app, 4));
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: true },
    );
    app.host_input.set_keyboard(HostButton::A, true);
    live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    assert!(app.tas_control.live_frame_in_flight());

    app.handle_focus_change(false);
    app.handle_tas_editor_window_event(WindowEvent::Focused(editor_focused));
    app.apply_focus_state();
    assert_eq!(app.window_focused, editor_focused);
    assert!(app.realtime_tas_recording_waiting_for_game_input());
    app.pump_realtime_tas_recording();
    let (response, _) = receive_live_frame(&app);
    assert!(app.consume_tas_control_response(response).is_none());
    assert_recording_project(&app, 5);
    let accepted_project = app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap()
        .project()
        .encode()
        .unwrap();
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(25));
        app.pump_realtime_tas_recording();
        app.drain_emu_responses();
        assert!(linked_at(&app, 5));
        assert!(app.realtime_tas_recording_active());
        assert!(app.realtime_tas_recording_waiting_for_game_input());
        assert_eq!(
            app.debug_windows
                .tas_editor
                .active_session()
                .unwrap()
                .project()
                .encode()
                .unwrap(),
            accepted_project
        );
    }

    app.handle_tas_editor_window_event(WindowEvent::Focused(false));
    app.handle_focus_change(true);
    app.apply_focus_state();
    assert!(!app.realtime_tas_recording_waiting_for_game_input());
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.tas_control.live_frame_in_flight() && Instant::now() < deadline {
        app.pump_realtime_tas_recording();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(app.tas_control.live_frame_in_flight());
    app.handle_focus_change(false);
    app.apply_focus_state();
    app.pump_realtime_tas_recording();
    let response = app.emu_thread.as_ref().unwrap().recv_checked().unwrap();
    let EmuResponse::TasFrameAdvanced {
        executed_project_frames: 6,
        state_sha256,
        ..
    } = &response
    else {
        panic!("expected exactly one resumed recording frame");
    };
    let expected_state = *state_sha256;
    assert!(app.consume_tas_control_response(response).is_none());
    assert!(linked_at(&app, 6));
    let session = app.debug_windows.tas_editor.active_session().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 6);
    assert_eq!(session.selected_branch().input_at(4).players[0].buttons, 1);
    assert_eq!(
        session.selected_branch().input_at(5),
        crate::tas_project::TasInputFrame::default()
    );
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: false },
    );
    live_ok(&mut app, LiveCommand::TasDisconnect { keep: true });
    settle(&mut app, |app| {
        app.tas_control.state == TasControlState::Detached && !app.tas_control.readiness_pending()
    });
    assert_eq!(TasDigest::from_bytes(&capture_state(&app)), expected_state);
}

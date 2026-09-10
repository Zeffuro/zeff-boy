use std::time::{Duration, Instant};

use super::harness::{app_with_worker, live_ok};
use super::*;
use crate::app::App;
use crate::debug::TasEditorWindowState;
use crate::emu_backend::loader::DirectNesTasExecutionLoader;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasEditorSessionSource, TasInputFrame, TasProject,
};

struct AcceptedRecording {
    app: App,
    _root: crate::test_support::TestDirectory,
    manual_path: std::path::PathBuf,
    original_manual_bytes: Vec<u8>,
    project_id: String,
    accepted_project_bytes: Vec<u8>,
    accepted_native_state: Vec<u8>,
    autosave_generation: u64,
    autosave_path: std::path::PathBuf,
}

#[test]
fn accepted_live_recording_autosaves_recovers_reopens_and_replays_exact_native_state() {
    recover_accepted_live_recording("tas-live-recording-recovery", false);
}

#[test]
fn accepted_live_recording_recovery_skips_a_corrupt_newest_autosave() {
    recover_accepted_live_recording("tas-live-recording-recovery-corrupt-newest", true);
}

fn recover_accepted_live_recording(name: &str, corrupt_newest: bool) {
    let mut recording = accepted_live_recording(name);

    if corrupt_newest {
        let autosaves = TasAutosaveStore::beside_manual_save(
            &recording.manual_path,
            TasAutosaveConfig::default(),
        )
        .unwrap();
        let accepted = TasProject::decode(&recording.accepted_project_bytes).unwrap();
        let newer = autosaves.save(&accepted).unwrap();
        std::fs::write(newer.path, b"corrupt newer TAS autosave").unwrap();
    }

    recording.app.debug_windows.tas_editor = TasEditorWindowState::new();
    live_ok(
        &mut recording.app,
        LiveCommand::TasOpenProject {
            path: recording.manual_path.clone(),
        },
    );
    recording.app.debug_windows.tas_editor.open_embedded();
    assert_eq!(
        std::fs::read(&recording.manual_path).unwrap(),
        recording.original_manual_bytes
    );

    let autosaves =
        TasAutosaveStore::beside_manual_save(&recording.manual_path, TasAutosaveConfig::default())
            .unwrap();
    let recovery = autosaves
        .recover_newest(&recording.project_id)
        .unwrap()
        .unwrap();
    assert_eq!(recovery.generation, recording.autosave_generation);
    assert_eq!(recovery.path, recording.autosave_path);
    assert_eq!(
        recovery.project.encode().unwrap(),
        recording.accepted_project_bytes
    );

    let context = egui::Context::default();
    context.global_style_mut(|style| style.animation_time = 0.0);
    let size = egui::vec2(1_024.0, 768.0);
    click_editor_button(
        &context,
        &mut recording.app.debug_windows.tas_editor,
        size,
        "Recovery",
    );
    click_editor_button(
        &context,
        &mut recording.app.debug_windows.tas_editor,
        size,
        "Recover newest autosave",
    );
    click_editor_button(
        &context,
        &mut recording.app.debug_windows.tas_editor,
        size,
        "Recover newest copy",
    );
    let recovered = recording
        .app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap();
    assert_eq!(recovered.project().project_id(), recording.project_id);
    assert_eq!(
        recovered.project().encode().unwrap(),
        recording.accepted_project_bytes
    );
    assert_eq!(recovered.source(), TasEditorSessionSource::Autosave);
    settle(&mut recording.app, |app| {
        app.tas_control_readiness_report().is_some_and(|report| {
            report.status == crate::app::tas_control::readiness::TasReadinessStatus::Ready
        })
    });

    live_ok(
        &mut recording.app,
        LiveCommand::TasSelectBoundary { boundary: 0 },
    );
    live_ok(
        &mut recording.app,
        LiveCommand::TasLink {
            at_end: true,
            record: false,
        },
    );
    settle(&mut recording.app, |app| {
        matches!(
            app.tas_control.state,
            TasControlState::AwaitingDecision {
                candidate_executed_project_frames: 2,
                ..
            }
        )
    });
    live_ok(
        &mut recording.app,
        LiveCommand::TasDisconnect { keep: true },
    );
    settle(&mut recording.app, |app| {
        app.tas_control.state == TasControlState::Detached && !app.tas_control.readiness_pending()
    });
    assert_eq!(
        capture_state(&recording.app),
        recording.accepted_native_state
    );
}

fn accepted_live_recording(name: &str) -> AcceptedRecording {
    let root = crate::test_support::test_directory(name).unwrap();
    let rom_path = root.path().join("recording.nes");
    std::fs::write(&rom_path, crate::test_support::build_nes_test_rom()).unwrap();
    let loader = DirectNesTasExecutionLoader::new(rom_path.clone(), Vec::new());
    let project = loader.create_project().unwrap();
    let project_id = project.project_id().to_owned();
    let manual_path = root.path().join("recording.ztas");
    project.save_atomic(&manual_path).unwrap();
    let original_manual_bytes = std::fs::read(&manual_path).unwrap();
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
        173,
        ActiveSystem::Nes,
        rom_path,
    );

    live_ok(
        &mut app,
        LiveCommand::TasOpenProject {
            path: manual_path.clone(),
        },
    );
    settle(&mut app, |app| {
        app.tas_control_readiness_report().is_some_and(|report| {
            report.status == crate::app::tas_control::readiness::TasReadinessStatus::Ready
        })
    });
    live_ok(
        &mut app,
        LiveCommand::TasLink {
            at_end: true,
            record: true,
        },
    );
    settle(&mut app, |app| {
        matches!(
            app.tas_control.state,
            TasControlState::AwaitingDecision {
                candidate_executed_project_frames: 1,
                ..
            }
        ) && app.tas_control.realtime_recording_active()
    });
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
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::TasSetRealtimeRecording { active: false },
    );
    settle(&mut app, |app| {
        matches!(
            app.tas_control.state,
            TasControlState::AwaitingDecision {
                candidate_executed_project_frames: 2,
                ..
            }
        )
    });
    let accepted_project_bytes = app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap()
        .project()
        .encode()
        .unwrap();
    assert_ne!(accepted_project_bytes, original_manual_bytes);
    let session = app.debug_windows.tas_editor.active_session().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 2);
    assert_eq!(session.cursor(), 2);
    assert_eq!(
        session.selected_branch().input_at(0),
        TasInputFrame::default()
    );
    assert_eq!(
        session.selected_branch().input_at(1).players[0].buttons,
        0x01
    );
    assert_eq!(session.selected_branch().input_at(1).players[0].dpad, 0);
    assert!(
        session.selected_branch().input_at(1).players[1..]
            .iter()
            .all(|player| *player == Default::default())
    );
    live_ok(&mut app, LiveCommand::TasDisconnect { keep: true });
    settle(&mut app, |app| {
        app.tas_control.state == TasControlState::Detached && !app.tas_control.readiness_pending()
    });
    let accepted_native_state = capture_state(&app);
    let autosave = app
        .debug_windows
        .tas_editor
        .autosave_before_shutdown()
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(&manual_path).unwrap(), original_manual_bytes);

    AcceptedRecording {
        app,
        _root: root,
        manual_path,
        original_manual_bytes,
        project_id,
        accepted_project_bytes,
        accepted_native_state,
        autosave_generation: autosave.generation,
        autosave_path: autosave.path,
    }
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
        "TAS recording recovery timed out: {:?}",
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

fn click_editor_button(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    label: &str,
) {
    let output = render_editor_content(context, state, size, Vec::new());
    let position = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + egui::vec2(8.0, text.galley.size().y * 0.5))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("could not find TAS editor button {label:?}"));
    render_editor_content(context, state, size, pointer_events(position, true));
    render_editor_content(context, state, size, pointer_events(position, false));
}

fn render_editor_content(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..egui::RawInput::default()
        },
        |ui| {
            assert!(crate::debug::draw_tas_editor_content(ui, state).is_none());
        },
    )
}

fn pointer_events(position: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

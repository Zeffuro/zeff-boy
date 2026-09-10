use super::*;
use crate::app::tas_control::tests::harness::app_with_worker;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::DirectNesTasExecutionLoader;
use crate::emu_thread::EmuThread;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasEditorSession, TasInputFrame, TasSeekStateCache,
};

fn session(
    project: crate::tas_project::TasProject,
    root: &std::path::Path,
    name: &str,
) -> TasEditorSession {
    let manual_path = root.join(format!("{name}.ztas"));
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.join(format!("{name}-seek-cache"))).unwrap();
    TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap()
}

fn refresh_both(app: &mut App) {
    app.refresh_tas_control_readiness();
    let _ = app.detached_tas_editor_live_status();
}

#[test]
fn live_validation_cache_skips_idle_refreshes_and_recomputes_for_project_changes() {
    let root = crate::test_support::test_directory("tas-live-validation-cache").unwrap();
    let rom_path = root.path().join("game.nes");
    std::fs::write(&rom_path, crate::test_support::build_nes_test_rom()).unwrap();
    let loader = DirectNesTasExecutionLoader::new(rom_path.clone(), Vec::new());
    let mut project = loader.create_project().unwrap();
    project
        .edit_transaction(|edit| edit.fork_branch("main", 0, "alternate", "Alternate"))
        .unwrap();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let initial_session = session(project, root.path(), "initial");
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 111, ActiveSystem::Nes, rom_path.clone());
    app.debug_windows
        .tas_editor
        .install_verified_export_session(initial_session.clone());

    refresh_both(&mut app);
    assert_eq!(app.tas_editor_live_validation_recomputations(), (1, 1));
    assert!(matches!(
        app.detached_tas_editor_live_status(),
        crate::debug::TasEditorLiveStatus::Unavailable(reason) if reason == "Checking TAS readiness…"
    ));

    refresh_both(&mut app);
    assert_eq!(app.tas_editor_live_validation_recomputations(), (1, 1));

    app.debug_windows
        .tas_editor
        .install_verified_export_session(initial_session.clone());
    refresh_both(&mut app);
    assert_eq!(app.tas_editor_live_validation_recomputations(), (1, 1));

    app.rom_info.source_path = Some(root.path().join("wrong.gb"));
    assert!(matches!(
        app.detached_tas_editor_live_status(),
        crate::debug::TasEditorLiveStatus::Unavailable(reason)
            if reason == "The loaded game does not match this direct TAS profile"
    ));
    assert_eq!(app.tas_editor_live_validation_recomputations(), (1, 1));
    app.rom_info.source_path = Some(rom_path.clone());
    assert!(matches!(
        app.detached_tas_editor_live_status(),
        crate::debug::TasEditorLiveStatus::Unavailable(reason) if reason == "Checking TAS readiness…"
    ));
    assert_eq!(app.tas_editor_live_validation_recomputations(), (1, 1));

    let mut branch_session = initial_session.clone();
    branch_session.select_branch("alternate").unwrap();
    app.debug_windows
        .tas_editor
        .install_verified_export_session(branch_session.clone());
    refresh_both(&mut app);
    assert_eq!(app.tas_editor_live_validation_recomputations(), (2, 2));

    let mut edited_session = branch_session.clone();
    edited_session
        .edit_transaction(|edit| edit.insert_frames("alternate", 0, 1))
        .unwrap();
    app.debug_windows
        .tas_editor
        .install_verified_export_session(edited_session);
    refresh_both(&mut app);
    assert_eq!(app.tas_editor_live_validation_recomputations(), (3, 3));

    let mut undone_session = branch_session.clone();
    undone_session
        .edit_transaction(|edit| edit.insert_frames("alternate", 0, 1))
        .unwrap();
    assert!(undone_session.undo().unwrap());
    app.debug_windows
        .tas_editor
        .install_verified_export_session(undone_session.clone());
    refresh_both(&mut app);
    assert_eq!(app.tas_editor_live_validation_recomputations(), (4, 4));
    assert!(undone_session.redo().unwrap());
    app.debug_windows
        .tas_editor
        .install_verified_export_session(undone_session);
    refresh_both(&mut app);
    assert_eq!(app.tas_editor_live_validation_recomputations(), (5, 5));

    let mut invalid_scope_session = initial_session.clone();
    let mut input = TasInputFrame::default();
    input.players[2].buttons = 1;
    invalid_scope_session
        .edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))
        .unwrap();
    app.debug_windows
        .tas_editor
        .install_verified_export_session(invalid_scope_session);
    assert!(matches!(
        app.detached_tas_editor_live_status(),
        crate::debug::TasEditorLiveStatus::Unavailable(reason)
            if reason == "The loaded game does not match this direct TAS profile"
    ));
    assert_eq!(app.tas_editor_live_validation_recomputations(), (6, 6));

    let mut replacement_project = loader.create_project().unwrap();
    replacement_project
        .edit_transaction(|edit| edit.insert_frames("main", 0, 2))
        .unwrap();
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session(replacement_project, root.path(), "replacement"));
    refresh_both(&mut app);
    assert_eq!(app.tas_editor_live_validation_recomputations(), (7, 7));
}

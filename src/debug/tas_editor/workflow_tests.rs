use anyhow::Result;

use super::*;
use crate::emu_backend::loader::DirectNesTasExecutionLoader;
use crate::tas_project::TasEditorExecutionAttachment;

#[test]
fn create_edit_preview_save_recover_and_reopen_flow() -> Result<()> {
    let root = crate::test_support::test_directory("tas-editor-complete-flow")?;
    let rom_path = root.path().join("game.nes");
    let project_path = root.path().join("movie.ztas");
    std::fs::write(&rom_path, crate::test_support::build_nes_test_rom())?;

    let loader = DirectNesTasExecutionLoader::new(rom_path.clone(), Vec::new());
    let mut state = TasEditorWindowState::with_seek_cache_root(root.path().join("seek-a"));
    state.attach_execution(TasEditorExecutionAttachment::Available(Box::new(loader)));
    assert_eq!(
        state.execution_availability,
        TasEditorExecutionAvailability::GameReady
    );

    DirectNesTasExecutionLoader::new(rom_path.clone(), Vec::new())
        .create_project_file(&project_path)?;
    state.open_project(project_path.clone())?;
    state.attach_execution(TasEditorExecutionAttachment::Available(Box::new(
        DirectNesTasExecutionLoader::new(rom_path.clone(), Vec::new()),
    )));
    assert_eq!(
        state.execution_availability,
        TasEditorExecutionAvailability::Ready
    );
    state.reduce(TasEditorAction::Autosave)?;
    assert!(
        state
            .session
            .as_ref()
            .unwrap()
            .last_autosaved_generation()
            .is_some()
    );

    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 0,
        player: 0,
        field: DigitalField::Buttons,
        mask: 1,
    })?;
    state.reduce(TasEditorAction::ExecuteSeek(
        execution_preview::selected_frame_playback_target(0, 1),
    ))?;
    assert_eq!(state.session.as_ref().unwrap().cursor(), 1);
    assert!(state.execution_preview.exact_frame().is_some());
    state.reduce(TasEditorAction::SaveManual)?;

    state.reduce(TasEditorAction::InsertNeutralFrames {
        cursor: 1,
        count: 1,
    })?;
    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 1,
        player: 0,
        field: DigitalField::Dpad,
        mask: 1 << 3,
    })?;
    state.reduce(TasEditorAction::Autosave)?;
    let autosaved = state.session.as_ref().unwrap().project().encode()?;

    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 1,
        player: 0,
        field: DigitalField::Buttons,
        mask: 1 << 1,
    })?;
    assert_ne!(
        state.session.as_ref().unwrap().project().encode()?,
        autosaved
    );
    state.reduce(TasEditorAction::RecoverAutosave)?;
    assert_eq!(
        state.session.as_ref().unwrap().project().encode()?,
        autosaved
    );
    state.reduce(TasEditorAction::SaveManual)?;

    let mut reopened = TasEditorWindowState::with_seek_cache_root(root.path().join("seek-b"));
    reopened.open_project(project_path)?;
    reopened.attach_execution(TasEditorExecutionAttachment::Available(Box::new(
        DirectNesTasExecutionLoader::new(rom_path, Vec::new()),
    )));
    let session = reopened.session.as_ref().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 2);
    assert_eq!(session.selected_branch().input_at(0).players[0].buttons, 1);
    assert_eq!(
        session.selected_branch().input_at(1).players[0].dpad,
        1 << 3
    );
    reopened.reduce(TasEditorAction::ExecuteSeek(2))?;
    assert!(reopened.execution_preview.exact_frame().is_some());

    Ok(())
}

#[test]
fn record_rows_save_and_reopen_flow() -> Result<()> {
    let (_root, mut state) = tests::state_with_project(1);
    let project_path = state.session.as_ref().unwrap().manual_path().to_owned();

    state.reduce(TasEditorAction::StartRecordingAtEnd)?;
    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 1,
        player: 0,
        field: DigitalField::Buttons,
        mask: 1,
    })?;
    state.reduce(TasEditorAction::CaptureRecordingFrame)?;
    state.reduce(TasEditorAction::CaptureRecordingFrame)?;
    state.reduce(TasEditorAction::StopRecording)?;
    state.reduce(TasEditorAction::SaveManual)?;

    let mut reopened =
        TasEditorWindowState::with_seek_cache_root(_root.path().join("recording-seek"));
    reopened.open_project(project_path)?;
    let session = reopened.session.as_ref().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 3);
    assert_eq!(session.selected_branch().input_at(1).players[0].buttons, 1);
    assert_eq!(
        session.selected_branch().input_at(2),
        crate::tas_project::TasInputFrame::default()
    );

    Ok(())
}

#[test]
fn project_navigation_requires_an_explicit_dirty_choice() -> Result<()> {
    let (_root, mut state) = tests::state_with_project(1);
    assert_eq!(
        state.begin_file_request(TasEditorFileRequest::OpenProject),
        Some(TasEditorFileRequest::OpenProject)
    );

    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 0,
        player: 0,
        field: DigitalField::Buttons,
        mask: 1,
    })?;
    assert_eq!(
        state.begin_file_request(TasEditorFileRequest::NewProject),
        None
    );
    assert_eq!(
        state.pending_file_request,
        Some(TasEditorFileRequest::NewProject)
    );
    state.reduce(TasEditorAction::CancelFileRequest)?;
    assert_eq!(state.pending_file_request, None);
    assert!(state.session.as_ref().unwrap().is_dirty());

    state.begin_file_request(TasEditorFileRequest::OpenProject);
    state.reduce(TasEditorAction::ContinueFileRequest { save: true })?;
    assert_eq!(
        state.ready_file_request.take(),
        Some(TasEditorFileRequest::OpenProject)
    );
    assert!(!state.session.as_ref().unwrap().is_dirty());

    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 0,
        player: 0,
        field: DigitalField::Dpad,
        mask: 1,
    })?;
    state.begin_file_request(TasEditorFileRequest::NewProject);
    state.reduce(TasEditorAction::ContinueFileRequest { save: false })?;
    assert_eq!(
        state.ready_file_request.take(),
        Some(TasEditorFileRequest::NewProject)
    );
    assert!(state.session.as_ref().unwrap().is_dirty());

    Ok(())
}

#[test]
fn editing_a_timeline_cell_selects_the_same_input_row() {
    let mut actions = Vec::new();
    timeline::queue_digital_toggle(&mut actions, 5, 0, DigitalField::Buttons, 1);
    assert_eq!(
        actions,
        [
            TasEditorAction::SelectTimelineFrame {
                frame: 5,
                extend_selection: false,
            },
            TasEditorAction::ToggleDigital {
                cursor: 5,
                player: 0,
                field: DigitalField::Buttons,
                mask: 1,
            },
        ]
    );

    actions.clear();
    timeline::queue_digital_toggle(&mut actions, 5, 0, DigitalField::Buttons, 1);
    assert_eq!(
        actions,
        [
            TasEditorAction::SelectTimelineFrame {
                frame: 5,
                extend_selection: false,
            },
            TasEditorAction::ToggleDigital {
                cursor: 5,
                player: 0,
                field: DigitalField::Buttons,
                mask: 1,
            },
        ]
    );
}

#[test]
fn keyboard_navigation_moves_selection_without_moving_the_linked_game() -> Result<()> {
    use super::action::TasTimelineNavigation;

    let (_root, mut state) = tests::state_with_project(6);
    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 4,
        recording_available: true,
    });
    state.reduce(TasEditorAction::SelectTimelineFrame {
        frame: 2,
        extend_selection: false,
    })?;
    state.reduce(TasEditorAction::NavigateTimelineSelection {
        navigation: TasTimelineNavigation::Next,
        extend_selection: true,
    })?;
    assert_eq!(state.session.as_ref().unwrap().cursor(), 3);
    assert_eq!(
        state
            .timeline_selection
            .selected_range(state.session.as_ref().unwrap()),
        Some((2, 4))
    );
    assert_eq!(state.live_status.execution_boundary(), Some(4));

    state.reduce(TasEditorAction::NavigateTimelineSelection {
        navigation: TasTimelineNavigation::Start,
        extend_selection: true,
    })?;
    assert_eq!(
        state
            .timeline_selection
            .selected_range(state.session.as_ref().unwrap()),
        Some((0, 3))
    );

    state.reduce(TasEditorAction::NavigateTimelineSelection {
        navigation: TasTimelineNavigation::End,
        extend_selection: true,
    })?;
    assert_eq!(
        state
            .timeline_selection
            .selected_range(state.session.as_ref().unwrap()),
        Some((2, 6))
    );

    state.reduce(TasEditorAction::NavigateTimelineSelection {
        navigation: TasTimelineNavigation::End,
        extend_selection: false,
    })?;
    assert_eq!(state.session.as_ref().unwrap().cursor(), 6);
    assert_eq!(
        state
            .timeline_selection
            .selected_range(state.session.as_ref().unwrap()),
        None
    );
    Ok(())
}

#[test]
fn range_selection_and_go_to_selection_keep_edits_and_live_actions_separate() -> Result<()> {
    let (_root, mut state) = tests::state_with_project(6);
    let before = state.session.as_ref().unwrap().project().encode()?;
    let undo_count = state.session.as_ref().unwrap().undo_count();

    state.reduce(TasEditorAction::SelectTimelineRange {
        anchor: 4,
        active: 1,
    })?;
    assert_eq!(state.session.as_ref().unwrap().cursor(), 1);
    assert_eq!(
        state
            .timeline_selection
            .selected_range(state.session.as_ref().unwrap()),
        Some((1, 5))
    );
    assert_eq!(state.session.as_ref().unwrap().project().encode()?, before);
    assert_eq!(state.session.as_ref().unwrap().undo_count(), undo_count);

    state.reduce(TasEditorAction::RequestLiveGoToSelection)?;
    assert_eq!(state.take_pending_host_request(), None);

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 3,
        recording_available: true,
    });
    state.reduce(TasEditorAction::RequestLiveGoToSelection)?;
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::GoToSelection
        ))
    );

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 1,
        recording_available: true,
    });
    state.reduce(TasEditorAction::RequestLiveGoToSelection)?;
    assert_eq!(state.take_pending_host_request(), None);
    Ok(())
}

#[test]
fn shutdown_autosave_captures_the_latest_dirty_project() -> Result<()> {
    let (_root, mut state) = tests::state_with_project(1);
    assert!(state.autosave_before_shutdown()?.is_none());
    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 0,
        player: 0,
        field: DigitalField::Buttons,
        mask: 1,
    })?;
    let saved = state.autosave_before_shutdown()?.unwrap();
    assert!(saved.path.exists());
    assert!(state.autosave_before_shutdown()?.is_none());
    Ok(())
}

#[test]
fn autosave_recovery_confirmation_can_be_cancelled_without_changes() -> Result<()> {
    let (_root, mut state) = tests::state_with_project(1);
    let before = state.session.as_ref().unwrap().project().encode()?;
    state.pending_autosave_recovery = true;
    state.reduce(TasEditorAction::CancelAutosaveRecovery)?;
    assert!(!state.pending_autosave_recovery);
    assert_eq!(state.session.as_ref().unwrap().project().encode()?, before);
    Ok(())
}

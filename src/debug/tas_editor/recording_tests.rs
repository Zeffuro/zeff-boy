use anyhow::Result;

use super::*;

fn fill_undo_history(state: &mut TasEditorWindowState, count: usize) -> Result<()> {
    for _ in 0..count {
        state.reduce(TasEditorAction::ToggleDigital {
            cursor: 0,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })?;
    }
    Ok(())
}

#[test]
fn recording_draft_at_a_full_history_restores_the_pre_draft_session() -> Result<()> {
    let (_root, mut state) = tests::state_with_project(1);
    let initial = state.session.as_ref().unwrap().project().encode()?;
    fill_undo_history(&mut state, 32)?;
    let before = state.session.as_ref().unwrap().project().encode()?;
    assert_eq!(state.session.as_ref().unwrap().undo_count(), 32);

    state.reduce(TasEditorAction::StartRecordingAtEnd)?;
    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 1,
        player: 0,
        field: DigitalField::Buttons,
        mask: 1,
    })?;
    state.reduce(TasEditorAction::StopRecording)?;

    let session = state.session.as_mut().unwrap();
    assert_eq!(session.project().encode()?, before);
    assert_eq!(session.selected_branch().frame_count(), 1);
    assert_eq!(session.cursor(), 0);
    assert_eq!(session.undo_count(), 32);
    assert_eq!(session.redo_count(), 0);
    for _ in 0..32 {
        assert!(session.undo()?);
    }
    assert!(!session.can_undo());
    assert_eq!(session.project().encode()?, initial);
    Ok(())
}

#[test]
fn recording_draft_after_the_last_history_slot_removes_both_row_and_input() -> Result<()> {
    let (_root, mut state) = tests::state_with_project(1);
    fill_undo_history(&mut state, 31)?;
    let before = state.session.as_ref().unwrap().project().encode()?;

    state.reduce(TasEditorAction::StartRecordingAtEnd)?;
    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 1,
        player: 0,
        field: DigitalField::Dpad,
        mask: 1,
    })?;
    state.reduce(TasEditorAction::StopRecording)?;

    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().encode()?, before);
    assert_eq!(session.selected_branch().frame_count(), 1);
    assert_eq!(session.undo_count(), 31);
    assert_eq!(session.redo_count(), 0);
    Ok(())
}

#[test]
fn recording_draft_restores_a_preexisting_redo_path() -> Result<()> {
    let (_root, mut state) = tests::state_with_project(1);
    fill_undo_history(&mut state, 2)?;
    state.reduce(TasEditorAction::Undo)?;
    let before = state.session.as_ref().unwrap().project().encode()?;
    assert_eq!(state.session.as_ref().unwrap().undo_count(), 1);
    assert_eq!(state.session.as_ref().unwrap().redo_count(), 1);

    state.reduce(TasEditorAction::StartRecordingAtEnd)?;
    state.reduce(TasEditorAction::StopRecording)?;

    let session = state.session.as_mut().unwrap();
    assert_eq!(session.project().encode()?, before);
    assert_eq!(session.cursor(), 0);
    assert_eq!(session.undo_count(), 1);
    assert_eq!(session.redo_count(), 1);
    assert!(session.redo()?);
    assert_eq!(session.selected_branch().input_at(0).players[0].buttons, 0);
    Ok(())
}

#[test]
fn recording_draft_remains_available_after_a_row_mismatch() -> Result<()> {
    let (_root, mut state) = tests::state_with_project(1);
    let before = state.session.as_ref().unwrap().project().encode()?;

    state.reduce(TasEditorAction::StartRecordingAtEnd)?;
    state.session.as_mut().unwrap().set_cursor(0)?;
    assert!(
        state
            .reduce(TasEditorAction::CaptureRecordingFrame)
            .is_err()
    );
    assert!(state.recording.is_some());

    state.reduce(TasEditorAction::StopRecording)?;
    assert!(state.recording.is_none());
    assert_eq!(state.session.as_ref().unwrap().project().encode()?, before);
    Ok(())
}

#[test]
fn recording_draft_shutdown_autosave_recovers_only_accepted_project_bytes() -> Result<()> {
    let (root, mut state) = tests::state_with_project(1);
    fill_undo_history(&mut state, 32)?;
    let expected = state.session.as_ref().unwrap().project().encode()?;
    let manual_path = state.session.as_ref().unwrap().manual_path().to_owned();

    state.reduce(TasEditorAction::StartRecordingAtEnd)?;
    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 1,
        player: 0,
        field: DigitalField::Buttons,
        mask: 1,
    })?;
    assert!(state.autosave_before_shutdown()?.is_some());
    assert!(state.recording.is_none());

    let mut recovered =
        TasEditorWindowState::with_seek_cache_root(root.path().join("recovered-seek"));
    recovered.open_project(manual_path)?;
    recovered.reduce(TasEditorAction::RecoverAutosave)?;
    assert_eq!(
        recovered.session.as_ref().unwrap().project().encode()?,
        expected
    );
    assert_eq!(
        recovered
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .frame_count(),
        1
    );
    Ok(())
}

#[test]
fn recording_next_draft_keeps_the_accepted_row_and_discards_only_the_final_draft() -> Result<()> {
    let (_root, mut state) = tests::state_with_project(1);

    state.reduce(TasEditorAction::StartRecordingAtEnd)?;
    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 1,
        player: 0,
        field: DigitalField::Buttons,
        mask: 1,
    })?;
    state.reduce(TasEditorAction::CaptureRecordingFrame)?;
    state.reduce(TasEditorAction::ToggleDigital {
        cursor: 2,
        player: 0,
        field: DigitalField::Dpad,
        mask: 1,
    })?;
    state.reduce(TasEditorAction::StopRecording)?;

    let session = state.session.as_ref().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 2);
    assert_eq!(session.selected_branch().input_at(1).players[0].buttons, 1);
    assert_eq!(session.cursor(), 1);
    assert_eq!(session.undo_count(), 2);
    Ok(())
}

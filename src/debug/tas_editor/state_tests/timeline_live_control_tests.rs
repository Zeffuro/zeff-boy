use super::super::action::TasTimelineNavigation;
use super::{
    TasEditorAction, TasEditorHostRequest, TasEditorLiveAction, TasEditorLiveStatus,
    state_with_project,
};
use crate::live_control::TasDigitalInput;

#[test]
fn timeline_cursor_changes_discard_recording_drafts_without_history() {
    let cases = [
        (
            TasEditorAction::SelectTimelineFrame {
                frame: 0,
                extend_selection: false,
            },
            0,
            Some((0, 1)),
        ),
        (
            TasEditorAction::SelectTimelineRange {
                anchor: 1,
                active: 0,
            },
            0,
            Some((0, 2)),
        ),
        (
            TasEditorAction::NavigateTimelineSelection {
                navigation: TasTimelineNavigation::Previous,
                extend_selection: false,
            },
            1,
            Some((1, 2)),
        ),
    ];

    for (action, expected_cursor, expected_range) in cases {
        let (_root, mut state) = state_with_project(2);
        let session = state.session.as_ref().unwrap();
        let before = session.project().encode().unwrap();
        let before_hash = session.project_content_sha256();
        let before_undo = session.undo_count();
        let before_redo = session.redo_count();
        let before_dirty = session.is_dirty();

        state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();
        assert!(state.recording.is_some());
        assert_eq!(
            state
                .session
                .as_ref()
                .unwrap()
                .selected_branch()
                .frame_count(),
            3
        );

        state.reduce(action).unwrap();

        assert!(state.recording.is_none());
        let session = state.session.as_ref().unwrap();
        assert_eq!(session.project().encode().unwrap(), before);
        assert_eq!(session.project_content_sha256(), before_hash);
        assert_eq!(session.undo_count(), before_undo);
        assert_eq!(session.redo_count(), before_redo);
        assert_eq!(session.is_dirty(), before_dirty);
        assert_eq!(session.cursor(), expected_cursor);
        assert_eq!(
            state.timeline_selection.selected_range(session),
            expected_range
        );
    }
}

#[test]
fn live_control_range_selection_rolls_back_drafts_and_uses_the_active_endpoint() {
    let (_root, mut state) = state_with_project(3);
    let session = state.session.as_ref().unwrap();
    let before = session.project().encode().unwrap();
    let before_hash = session.project_content_sha256();
    let before_undo = session.undo_count();
    let before_redo = session.redo_count();

    state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();
    state.select_input_range_for_live_control(0, 3).unwrap();

    assert!(state.recording.is_none());
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.project_content_sha256(), before_hash);
    assert_eq!(session.undo_count(), before_undo);
    assert_eq!(session.redo_count(), before_redo);
    assert_eq!(session.cursor(), 2);
    assert_eq!(
        state.timeline_selection.selected_range(session),
        Some((0, 3))
    );
}

#[test]
fn ui_and_live_control_ranges_share_selection_and_cursor_semantics() {
    let (_ui_root, mut ui_state) = state_with_project(6);
    let (_live_root, mut live_state) = state_with_project(6);

    ui_state
        .reduce(TasEditorAction::SelectTimelineRange {
            anchor: 1,
            active: 4,
        })
        .unwrap();
    live_state
        .select_input_range_for_live_control(1, 5)
        .unwrap();

    let ui_session = ui_state.session.as_ref().unwrap();
    let live_session = live_state.session.as_ref().unwrap();
    assert_eq!(ui_session.cursor(), 4);
    assert_eq!(live_session.cursor(), ui_session.cursor());
    assert_eq!(
        live_state.timeline_selection.selected_range(live_session),
        ui_state.timeline_selection.selected_range(ui_session)
    );
    assert_eq!(live_session.undo_count(), ui_session.undo_count());
    assert_eq!(live_session.redo_count(), ui_session.redo_count());
}

#[test]
fn live_authority_rejects_live_control_range_selection_without_mutation() {
    let (_root, mut state) = state_with_project(2);
    state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();
    state.set_live_status(TasEditorLiveStatus::Staging {
        completed: 0,
        total: 1,
    });
    let session = state.session.as_ref().unwrap();
    let before = session.project().encode().unwrap();
    let before_hash = session.project_content_sha256();
    let before_cursor = session.cursor();
    let before_undo = session.undo_count();
    let before_redo = session.redo_count();

    let error = state.select_input_range_for_live_control(0, 2).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("finish the live game decision before changing the TAS project")
    );
    assert!(state.recording.is_some());
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.project_content_sha256(), before_hash);
    assert_eq!(session.cursor(), before_cursor);
    assert_eq!(session.undo_count(), before_undo);
    assert_eq!(session.redo_count(), before_redo);
}

#[test]
fn live_control_digital_set_is_idempotent_and_reconstructs_only_after_change() {
    let (_root, mut state) = state_with_project(3);
    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 0,
        recording_available: true,
    });
    let before_undo = state.session.as_ref().unwrap().undo_count();

    state
        .set_digital_input_for_live_control(1, 5, TasDigitalInput::Buttons(1), false)
        .unwrap();
    assert_eq!(state.session.as_ref().unwrap().undo_count(), before_undo);
    assert_eq!(state.take_pending_host_request(), None);

    state
        .set_digital_input_for_live_control(1, 5, TasDigitalInput::Buttons(1), true)
        .unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().undo_count(),
        before_undo + 1
    );
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::ReconstructAfterEdit { start: 1, end: 2 }
        ))
    );

    state
        .set_digital_input_for_live_control(1, 5, TasDigitalInput::Buttons(1), true)
        .unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().undo_count(),
        before_undo + 1
    );
    assert_eq!(state.take_pending_host_request(), None);
}

#[test]
fn live_control_digital_set_rejects_controls_outside_the_project_columns() {
    let (_root, mut state) = state_with_project(1);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();

    let error = state
        .set_digital_input_for_live_control(0, 1, TasDigitalInput::Dpad(1 << 7), true)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("does not declare that digital control")
    );
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
    assert_eq!(state.take_pending_host_request(), None);
}

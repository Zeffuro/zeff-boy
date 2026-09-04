use super::*;
use crate::tas_project::{TasControllerInput, TasInputFrame};

fn input(buttons: u8) -> TasInputFrame {
    TasInputFrame {
        players: [TasControllerInput { buttons, dpad: 0 }; 5],
        ..TasInputFrame::default()
    }
}

fn set_input(state: &mut TasEditorWindowState, start: u64, length: u64, input: TasInputFrame) {
    let session = state.session.as_mut().unwrap();
    let branch_id = session.selected_branch_id().to_owned();
    session
        .edit_transaction(move |edit| edit.set_input_range(&branch_id, start, length, input))
        .unwrap();
}

fn copy_selection(state: &TasEditorWindowState, start: u64, end: u64) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    let branch_id = session.selected_branch_id().to_owned();
    let pattern = session
        .selected_branch()
        .input_pattern(start, end - start)
        .unwrap();
    TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::copy_selection(
        session.project_content_sha256(),
        branch_id.clone(),
        session.project().branch_movie_sha256(&branch_id).unwrap(),
        start,
        pattern,
        (start, end),
    ))
}

fn select_range(state: &mut TasEditorWindowState, start: u64, end: u64) {
    let session = state.session.as_ref().unwrap();
    state.timeline_selection.select_range(session, start, end);
}

fn selection_snapshot(state: &mut TasEditorWindowState) -> Option<(String, u64, u64)> {
    let session = state.session.as_ref().unwrap();
    state
        .timeline_selection
        .snapshot(session)
        .map(|selection| (selection.branch_id, selection.start, selection.end))
}

#[test]
fn sparse_selection_copy_is_immutable_and_paste_preserves_neutral_gaps() {
    let (_root, mut state) = tests::state_with_project(10);
    let first = input(1);
    let second = input(2);
    set_input(&mut state, 0, 1, first);
    set_input(&mut state, 2, 1, second);
    select_range(&mut state, 0, 4);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    state.reduce(copy_selection(&state, 0, 4)).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    set_input(&mut state, 0, 1, TasInputFrame::default());
    state.reduce(TasEditorAction::SelectCursor(4)).unwrap();
    let session = state.session.as_ref().unwrap();
    let action = input_clipboard::TasInputClipboardAction::paste_at_cursor(
        session.project_content_sha256(),
        "main".to_owned(),
        session.project().branch_movie_sha256("main").unwrap(),
        4,
        state.input_clipboard.generation(),
    );
    state
        .reduce(TasEditorAction::InputClipboard(action))
        .unwrap();
    let branch = state.session.as_ref().unwrap().selected_branch();
    assert_eq!(branch.input_at(4), first);
    assert_eq!(branch.input_at(5), TasInputFrame::default());
    assert_eq!(branch.input_at(6), second);
    assert_eq!(branch.input_at(7), TasInputFrame::default());
}

#[test]
fn sparse_paste_checks_every_non_neutral_generated_run() {
    let (_root, mut state) = tests::state_with_project(10);
    let regular = input(1);
    let unsupported = TasInputFrame {
        tilt_x_bits: 1,
        ..TasInputFrame::default()
    };
    set_input(&mut state, 0, 1, regular);
    set_input(&mut state, 2, 1, unsupported);
    select_range(&mut state, 0, 4);
    state.reduce(copy_selection(&state, 0, 4)).unwrap();
    state.reduce(TasEditorAction::SelectCursor(4)).unwrap();
    let (action, before) = {
        let session = state.session.as_ref().unwrap();
        (
            input_clipboard::TasInputClipboardAction::paste_at_cursor(
                session.project_content_sha256(),
                "main".to_owned(),
                session.project().branch_movie_sha256("main").unwrap(),
                4,
                state.input_clipboard.generation(),
            ),
            session.project().encode().unwrap(),
        )
    };
    assert!(
        state
            .reduce(TasEditorAction::InputClipboard(action))
            .is_err()
    );
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

#[test]
fn tiling_is_bounded_and_rejects_stale_or_invalid_selection() {
    let (_root, mut state) = tests::state_with_project(10);
    let first = input(1);
    let second = input(2);
    set_input(&mut state, 0, 1, first);
    set_input(&mut state, 2, 1, second);
    select_range(&mut state, 0, 4);
    state.reduce(copy_selection(&state, 0, 4)).unwrap();
    let session = state.session.as_ref().unwrap();
    let action = input_clipboard::TasInputClipboardAction::tile_selection(
        session.project_content_sha256(),
        "main".to_owned(),
        session.project().branch_movie_sha256("main").unwrap(),
        4,
        10,
        state.input_clipboard.generation(),
    );
    select_range(&mut state, 4, 10);
    state
        .reduce(TasEditorAction::InputClipboard(action))
        .unwrap();
    let branch = state.session.as_ref().unwrap().selected_branch();
    assert_eq!(branch.input_at(4), first);
    assert_eq!(branch.input_at(5), TasInputFrame::default());
    assert_eq!(branch.input_at(6), second);
    assert_eq!(branch.input_at(8), first);

    let session = state.session.as_ref().unwrap();
    let stale = input_clipboard::TasInputClipboardAction::tile_selection(
        session.project_content_sha256(),
        "main".to_owned(),
        session.project().branch_movie_sha256("main").unwrap(),
        4,
        10,
        state.input_clipboard.generation(),
    );
    select_range(&mut state, 5, 10);
    assert!(
        state
            .reduce(TasEditorAction::InputClipboard(stale))
            .is_err()
    );
}

#[test]
fn boundary_selection_collapses_rows_and_syncs_across_movie_edits() {
    let (_root, mut state) = tests::state_with_project(4);
    select_range(&mut state, 1, 3);
    state.reduce(TasEditorAction::SelectCursor(2)).unwrap();
    assert_eq!(
        selection_snapshot(&mut state),
        Some(("main".to_owned(), 2, 3))
    );
    set_input(&mut state, 0, 1, input(1));
    state
        .session
        .as_mut()
        .unwrap()
        .insert_neutral_frames(4, 1)
        .unwrap();
    assert_eq!(
        selection_snapshot(&mut state),
        Some(("main".to_owned(), 2, 3))
    );
    state
        .reduce(TasEditorAction::ForkBranch {
            id: "child".to_owned(),
            name: "Child".to_owned(),
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SelectBranch("child".to_owned()))
        .unwrap();
    assert_eq!(
        selection_snapshot(&mut state),
        Some(("child".to_owned(), 2, 3))
    );
}

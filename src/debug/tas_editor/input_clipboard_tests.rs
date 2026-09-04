use super::*;
use crate::tas_project::{
    TasCameraInput, TasControllerInput, TasDigest, TasInputFrame, TasZapperInput,
};
use zeff_emu_common::replay::{POCKET_CAMERA_FRAME_BYTES, ReplayEvent};

fn set_input(state: &mut TasEditorWindowState, start: u64, length: u64, input: TasInputFrame) {
    let session = state.session.as_mut().unwrap();
    let branch_id = session.selected_branch_id().to_owned();
    session
        .edit_transaction(move |edit| edit.set_input_range(&branch_id, start, length, input))
        .unwrap();
}

fn input(buttons: u8) -> TasInputFrame {
    TasInputFrame {
        players: [TasControllerInput { buttons, dpad: 0 }; 5],
        ..TasInputFrame::default()
    }
}

fn copy_constant(
    state: &TasEditorWindowState,
    start: u64,
    length: u64,
    input: TasInputFrame,
) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    TasEditorAction::InputClipboard(
        input_clipboard::TasInputClipboardAction::copy_constant(
            session.project_content_sha256(),
            session.selected_branch_id().to_owned(),
            session
                .project()
                .branch_movie_sha256(session.selected_branch_id())
                .unwrap(),
            start,
            length,
            input,
        )
        .unwrap(),
    )
}

fn paste_at_cursor(state: &TasEditorWindowState) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::paste_at_cursor(
        session.project_content_sha256(),
        session.selected_branch_id().to_owned(),
        session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap(),
        session.cursor(),
        state.input_clipboard.generation(),
    ))
}

fn insert_at_cursor(state: &TasEditorWindowState) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::insert_at_cursor(
        session.project_content_sha256(),
        session.selected_branch_id().to_owned(),
        session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap(),
        session.cursor(),
        state.input_clipboard.generation(),
    ))
}

fn copy_selected_frames(state: &mut TasEditorWindowState, start: u64, end: u64) {
    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: start,
            extend_selection: false,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: end - 1,
            extend_selection: true,
        })
        .unwrap();
    let session = state.session.as_ref().unwrap();
    let branch = session.selected_branch();
    let events = branch
        .events()
        .iter()
        .filter(|event| event.frame() >= start && event.frame() < end)
        .cloned()
        .collect();
    let action = input_clipboard::TasInputClipboardAction::copy_selection_with_events(
        session.project_content_sha256(),
        session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap(),
        branch.input_pattern(start, end - start).unwrap(),
        events,
        timeline_selection::TasInputSelection {
            branch_id: session.selected_branch_id().to_owned(),
            start,
            end,
        },
    );
    state
        .reduce(TasEditorAction::InputClipboard(action))
        .unwrap();
}

fn selected_input_range(state: &mut TasEditorWindowState) -> Option<(u64, u64)> {
    let session = state.session.as_ref().unwrap();
    state.timeline_selection.selected_range(session)
}

fn paste_with(
    project_sha256: TasDigest,
    branch_id: String,
    movie_sha256: TasDigest,
    cursor: u64,
    generation: u64,
) -> TasEditorAction {
    TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::paste_at_cursor(
        project_sha256,
        branch_id,
        movie_sha256,
        cursor,
        generation,
    ))
}

#[test]
fn retained_diff_copy_is_read_only_and_validates_its_whole_run() {
    let (_root, mut state) = tests::state_with_project(5);
    let pressed = input(1);
    set_input(&mut state, 1, 3, pressed);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    state.reduce(copy_constant(&state, 1, 3, pressed)).unwrap();
    assert!(state.input_clipboard.has_entry());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
    assert!(state.reduce(copy_constant(&state, 1, 4, pressed)).is_err());
    assert!(
        state
            .reduce(copy_constant(&state, 0, 2, TasInputFrame::default()))
            .is_err()
    );
}

#[test]
fn paste_is_fixed_length_and_rejects_a_stale_cursor_without_history() {
    let (_root, mut state) = tests::state_with_project(5);
    let pressed = input(1);
    set_input(&mut state, 0, 2, pressed);
    state.reduce(copy_constant(&state, 0, 2, pressed)).unwrap();
    state.reduce(TasEditorAction::SelectCursor(3)).unwrap();
    let before_frames = state
        .session
        .as_ref()
        .unwrap()
        .selected_branch()
        .frame_count();
    let before_history = state.session.as_ref().unwrap().undo_count();
    let before_rerecords = state.session.as_ref().unwrap().project().rerecord_count();
    state.reduce(paste_at_cursor(&state)).unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .frame_count(),
        before_frames
    );
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(3),
        pressed
    );
    assert_eq!(
        state.session.as_ref().unwrap().undo_count(),
        before_history + 1
    );
    assert_eq!(
        state.session.as_ref().unwrap().project().rerecord_count(),
        before_rerecords + 1
    );
    state.reduce(TasEditorAction::Autosave).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().last_autosaved_generation(),
        Some(state.session.as_ref().unwrap().project().edit_generation())
    );

    let no_op_history = state.session.as_ref().unwrap().undo_count();
    state.reduce(paste_at_cursor(&state)).unwrap();
    assert_eq!(state.session.as_ref().unwrap().undo_count(), no_op_history);

    let stale = paste_at_cursor(&state);
    state.reduce(TasEditorAction::SelectCursor(2)).unwrap();
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(stale).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

#[test]
fn linked_paste_reconstructs_once_after_the_complete_changed_range() {
    let (_root, mut state) = tests::state_with_project(5);
    let pressed = input(1);
    set_input(&mut state, 0, 2, pressed);
    state.reduce(copy_constant(&state, 0, 2, pressed)).unwrap();
    state.reduce(TasEditorAction::SelectCursor(3)).unwrap();
    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 1,
        recording_available: true,
    });

    state.reduce(paste_at_cursor(&state)).unwrap();
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::ReconstructAfterEdit { start: 3, end: 5 }
        ))
    );

    state.reduce(paste_at_cursor(&state)).unwrap();
    assert_eq!(state.take_pending_host_request(), None);
}

#[test]
fn timeline_selection_is_contiguous_and_clamps_an_end_anchor() {
    let (_root, mut state) = tests::state_with_project(6);

    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 2,
            extend_selection: false,
        })
        .unwrap();
    assert_eq!(selected_input_range(&mut state), Some((2, 3)));

    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 5,
            extend_selection: true,
        })
        .unwrap();
    assert_eq!(selected_input_range(&mut state), Some((2, 6)));

    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 1,
            extend_selection: true,
        })
        .unwrap();
    assert_eq!(selected_input_range(&mut state), Some((1, 3)));

    state
        .reduce(TasEditorAction::ClearTimelineSelection)
        .unwrap();
    assert_eq!(selected_input_range(&mut state), Some((1, 2)));

    state.reduce(TasEditorAction::SelectCursor(6)).unwrap();
    state
        .reduce(TasEditorAction::ClearTimelineSelection)
        .unwrap();
    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 4,
            extend_selection: true,
        })
        .unwrap();
    assert_eq!(selected_input_range(&mut state), Some((4, 6)));
}

#[test]
fn copied_frames_can_be_deleted_reinserted_and_undone_exactly() {
    let (_root, mut state) = tests::state_with_project(6);
    let first = input(1);
    let second = input(2);
    let third = input(4);
    set_input(&mut state, 1, 1, first);
    set_input(&mut state, 2, 1, second);
    set_input(&mut state, 3, 1, third);
    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 1,
            extend_selection: false,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 3,
            extend_selection: true,
        })
        .unwrap();
    let original_movie = {
        let session = state.session.as_ref().unwrap();
        session.project().branch_movie_sha256("main").unwrap()
    };
    let copied = {
        let session = state.session.as_ref().unwrap();
        session.selected_branch().input_pattern(1, 3).unwrap()
    };
    let session = state.session.as_ref().unwrap();
    let copy = input_clipboard::TasInputClipboardAction::copy_selection(
        session.project_content_sha256(),
        "main".to_owned(),
        session.project().branch_movie_sha256("main").unwrap(),
        1,
        copied,
        (1, 4),
    );
    state.reduce(TasEditorAction::InputClipboard(copy)).unwrap();

    state
        .reduce(TasEditorAction::DeleteFrames { start: 1, count: 3 })
        .unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .frame_count(),
        3
    );
    state.reduce(TasEditorAction::SelectCursor(1)).unwrap();
    state.reduce(insert_at_cursor(&state)).unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .frame_count(),
        6
    );
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .project()
            .branch_movie_sha256("main")
            .unwrap(),
        original_movie
    );
    assert_eq!(selected_input_range(&mut state), Some((1, 4)));

    state.reduce(TasEditorAction::Undo).unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .frame_count(),
        3
    );
    state.reduce(TasEditorAction::Undo).unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .project()
            .branch_movie_sha256("main")
            .unwrap(),
        original_movie
    );
}

#[test]
fn fixed_paste_replaces_fds_events_with_the_copied_relative_timeline() {
    let original_events = vec![
        ReplayEvent::FdsDiskSide { frame: 2, side: 2 },
        ReplayEvent::FdsDiskSide { frame: 4, side: 0 },
    ];
    let (_root, mut state) = event_tests::fds_state(6, original_events.clone());
    copy_selected_frames(&mut state, 1, 3);
    state.reduce(TasEditorAction::SelectCursor(3)).unwrap();
    state.reduce(paste_at_cursor(&state)).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().selected_branch().events(),
        &[
            ReplayEvent::FdsDiskSide { frame: 2, side: 2 },
            ReplayEvent::FdsDiskSide { frame: 4, side: 2 },
        ]
    );
    state.reduce(TasEditorAction::Undo).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().selected_branch().events(),
        original_events
    );
}

#[test]
fn inserted_clipboard_frames_shift_existing_fds_events_and_add_copied_events() {
    let original_events = vec![
        ReplayEvent::FdsDiskSide { frame: 2, side: 2 },
        ReplayEvent::FdsDiskSide { frame: 4, side: 0 },
    ];
    let (_root, mut state) = event_tests::fds_state(6, original_events.clone());
    copy_selected_frames(&mut state, 1, 3);
    state.reduce(TasEditorAction::SelectCursor(3)).unwrap();
    state.reduce(insert_at_cursor(&state)).unwrap();
    let branch = state.session.as_ref().unwrap().selected_branch();
    assert_eq!(branch.frame_count(), 8);
    assert_eq!(
        branch.events(),
        &[
            ReplayEvent::FdsDiskSide { frame: 2, side: 2 },
            ReplayEvent::FdsDiskSide { frame: 4, side: 2 },
            ReplayEvent::FdsDiskSide { frame: 6, side: 0 },
        ]
    );
    state.reduce(TasEditorAction::Undo).unwrap();
    let branch = state.session.as_ref().unwrap().selected_branch();
    assert_eq!(branch.frame_count(), 6);
    assert_eq!(branch.events(), original_events);
}

#[test]
fn clipboard_survives_history_but_open_and_recovery_clear_it() {
    let (root, mut state) = tests::state_with_project(4);
    let pressed = input(1);
    set_input(&mut state, 0, 1, pressed);
    state.reduce(copy_constant(&state, 0, 1, pressed)).unwrap();
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 2,
            player: 0,
            field: DigitalField::Dpad,
            mask: 1,
        })
        .unwrap();
    state.reduce(TasEditorAction::Undo).unwrap();
    state.reduce(TasEditorAction::Redo).unwrap();
    assert!(state.input_clipboard.has_entry());
    state
        .reduce(TasEditorAction::OpenProject(root.path().join("movie.ztas")))
        .unwrap();
    assert!(!state.input_clipboard.has_entry());
    set_input(&mut state, 0, 1, pressed);
    state.reduce(copy_constant(&state, 0, 1, pressed)).unwrap();
    state.reduce(TasEditorAction::Autosave).unwrap();
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 2,
            player: 0,
            field: DigitalField::Dpad,
            mask: 1,
        })
        .unwrap();
    state.reduce(TasEditorAction::RecoverAutosave).unwrap();
    assert!(!state.input_clipboard.has_entry());
}

#[test]
fn stale_generation_project_branch_movie_and_end_witnesses_are_atomic() {
    let (_root, mut state) = tests::state_with_project(4);
    let pressed = input(1);
    set_input(&mut state, 0, 2, pressed);
    state.reduce(copy_constant(&state, 0, 2, pressed)).unwrap();
    let stale_generation = paste_at_cursor(&state);
    state.reduce(copy_constant(&state, 0, 2, pressed)).unwrap();
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(stale_generation).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    let stale_project = paste_at_cursor(&state);
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 2,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(stale_project).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    let session = state.session.as_ref().unwrap();
    let project_sha256 = session.project_content_sha256();
    let movie_sha256 = session.project().branch_movie_sha256("main").unwrap();
    let generation = state.input_clipboard.generation();
    assert!(
        state
            .reduce(paste_with(
                project_sha256,
                "missing".to_owned(),
                movie_sha256,
                0,
                generation,
            ))
            .is_err()
    );
    assert!(
        state
            .reduce(paste_with(
                project_sha256,
                "main".to_owned(),
                TasDigest([0xA5; 32]),
                0,
                generation,
            ))
            .is_err()
    );
    assert!(
        state
            .reduce(paste_with(
                project_sha256,
                "main".to_owned(),
                movie_sha256,
                3,
                generation,
            ))
            .is_err()
    );
}

#[test]
fn neutral_clipboard_clears_legacy_special_input_but_new_special_values_are_guarded() {
    let legacy_tilt = TasInputFrame {
        tilt_x_bits: 1,
        ..TasInputFrame::default()
    };
    let (_root, mut state) = tests::state_with_project(3);
    set_input(&mut state, 0, 1, legacy_tilt);
    state
        .reduce(copy_constant(&state, 1, 1, TasInputFrame::default()))
        .unwrap();
    state.reduce(TasEditorAction::SelectCursor(0)).unwrap();
    state.reduce(paste_at_cursor(&state)).unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(0),
        TasInputFrame::default()
    );

    for unsupported in [
        legacy_tilt,
        TasInputFrame {
            zapper: TasZapperInput {
                enabled: true,
                trigger: true,
                ..TasZapperInput::default()
            },
            ..TasInputFrame::default()
        },
    ] {
        set_input(&mut state, 0, 1, unsupported);
        state
            .reduce(copy_constant(&state, 0, 1, unsupported))
            .unwrap();
        state.reduce(TasEditorAction::SelectCursor(1)).unwrap();
        let before = state.session.as_ref().unwrap().project().encode().unwrap();
        assert!(state.reduce(paste_at_cursor(&state)).is_err());
        assert_eq!(
            state.session.as_ref().unwrap().project().encode().unwrap(),
            before
        );
        state.reduce(TasEditorAction::SelectCursor(0)).unwrap();
        set_input(&mut state, 0, 1, TasInputFrame::default());
    }
}

#[test]
fn camera_clipboard_rejects_wrong_sized_and_missing_assets() {
    let bytes = vec![0x55; POCKET_CAMERA_FRAME_BYTES - 1];
    let digest = TasDigest::from_bytes(&bytes);
    let camera = TasInputFrame {
        camera: TasCameraInput::Blob(digest),
        ..TasInputFrame::default()
    };
    let mut assets = std::collections::BTreeMap::new();
    assets.insert(digest, bytes);
    let (_root, mut state) = special_input_tests::synthetic_state(
        "gb",
        &[special_input_editor::GAME_BOY_POCKET_CAMERA_DEVICE],
        camera,
        assets,
    );
    state.reduce(copy_constant(&state, 0, 1, camera)).unwrap();
    state.reduce(TasEditorAction::SelectCursor(1)).unwrap();
    assert!(state.reduce(paste_at_cursor(&state)).is_err());

    let bytes = vec![0xA5; POCKET_CAMERA_FRAME_BYTES];
    let digest = TasDigest::from_bytes(&bytes);
    let camera = TasInputFrame {
        camera: TasCameraInput::Blob(digest),
        ..TasInputFrame::default()
    };
    let mut assets = std::collections::BTreeMap::new();
    assets.insert(digest, bytes);
    let (_root, mut state) = special_input_tests::synthetic_state(
        "gb",
        &[special_input_editor::GAME_BOY_POCKET_CAMERA_DEVICE],
        camera,
        assets,
    );
    state.reduce(copy_constant(&state, 0, 1, camera)).unwrap();
    set_input(&mut state, 0, 1, TasInputFrame::default());
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(move |edit| {
            assert!(edit.remove_camera_asset(digest));
            Ok(())
        })
        .unwrap();
    state.reduce(TasEditorAction::SelectCursor(1)).unwrap();
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(paste_at_cursor(&state)).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

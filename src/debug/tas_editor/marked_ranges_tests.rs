use super::*;
use crate::tas_project::{
    TasControllerInput, TasDigest, TasDigitalInputMask, TasDigitalTransform, TasInputFrame,
};
use input_clipboard::marked_ranges::{TasMarkedRangesAction, TasMarkedRangesSnapshot};
use zeff_emu_common::replay::ReplayEvent;

fn snapshot(state: &mut TasEditorWindowState) -> TasMarkedRangesSnapshot {
    state
        .input_clipboard
        .marked_ranges
        .snapshot(state.session.as_ref().unwrap())
        .unwrap()
}

fn action(action: TasMarkedRangesAction) -> TasEditorAction {
    TasEditorAction::InputClipboard(input_clipboard::TasInputClipboardAction::MarkedRanges(
        action,
    ))
}

fn add(state: &mut TasEditorWindowState, start: u64, end: u64) -> anyhow::Result<()> {
    let session = state.session.as_ref().unwrap();
    state.timeline_selection.select_range(session, start, end);
    let selection = state.timeline_selection.snapshot(session).unwrap();
    let snapshot = snapshot(state);
    state
        .reduce(action(TasMarkedRangesAction::Add {
            snapshot,
            selection,
        }))
        .map(|_| ())
}

fn mask() -> TasDigitalInputMask {
    let mut mask = TasDigitalInputMask::default();
    mask.players[0] = TasControllerInput {
        buttons: 3,
        dpad: 0,
    };
    mask.players[1] = TasControllerInput {
        buttons: 0,
        dpad: 5,
    };
    mask
}

fn apply(state: &mut TasEditorWindowState, transform: TasDigitalTransform) -> TasEditorAction {
    let snapshot = snapshot(state);
    let session = state.session.as_ref().unwrap();
    action(TasMarkedRangesAction::Apply {
        snapshot,
        target_movie_sha256: session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap(),
        mask: mask(),
        transform,
    })
}

fn bytes(state: &TasEditorWindowState) -> Vec<u8> {
    state.session.as_ref().unwrap().project().encode().unwrap()
}

#[test]
fn marked_ranges_transform_each_interval_and_undo_redo_as_one_edit() {
    for transform in [
        TasDigitalTransform::Clear,
        TasDigitalTransform::Invert,
        TasDigitalTransform::Reverse,
    ] {
        let events = vec![
            ReplayEvent::FdsDiskSide { frame: 3, side: 1 },
            ReplayEvent::FdsDiskSide { frame: 7, side: 2 },
        ];
        let (_root, mut state) = digital_transform_tests::fds_two_player_state(10, events.clone());
        for frame in 0..10 {
            let mut input = TasInputFrame::default();
            input.players[0] = TasControllerInput {
                buttons: 12 | (frame as u8 % 4),
                dpad: frame as u8 % 16,
            };
            input.players[1] = TasControllerInput {
                buttons: frame as u8,
                dpad: (frame as u8 * 3) % 16,
            };
            input.players[2].buttons = frame as u8;
            state
                .session
                .as_mut()
                .unwrap()
                .edit_transaction(|edit| edit.set_input_range("main", frame, 1, input))
                .unwrap();
        }
        let before_inputs = (0..10)
            .map(|frame| {
                state
                    .session
                    .as_ref()
                    .unwrap()
                    .selected_branch()
                    .input_at(frame)
            })
            .collect::<Vec<_>>();
        let before = bytes(&state);
        let history = state.session.as_ref().unwrap().undo_count();
        add(&mut state, 1, 3).unwrap();
        add(&mut state, 6, 9).unwrap();
        let working_selection = state
            .timeline_selection
            .snapshot(state.session.as_ref().unwrap());
        assert_eq!(bytes(&state), before);
        let edit = apply(&mut state, transform);
        state.reduce(edit).unwrap();
        let after = bytes(&state);
        let session = state.session.as_ref().unwrap();
        assert_eq!(session.undo_count(), history + 1);
        assert_eq!(session.selected_branch().events(), events);
        assert_eq!(
            state.timeline_selection.snapshot(session),
            working_selection
        );
        for frame in 0..10 {
            let mut expected = before_inputs[frame];
            if let Some((start, end)) = [(1, 3), (6, 9)]
                .into_iter()
                .find(|&(start, end)| frame >= start && frame < end)
            {
                let mirrored = before_inputs[start + end - 1 - frame];
                match transform {
                    TasDigitalTransform::Clear => {
                        expected.players[0].buttons &= !3;
                        expected.players[1].dpad &= !5;
                    }
                    TasDigitalTransform::Invert => {
                        expected.players[0].buttons ^= 3;
                        expected.players[1].dpad ^= 5;
                    }
                    TasDigitalTransform::Reverse => {
                        expected.players[0].buttons =
                            (expected.players[0].buttons & !3) | (mirrored.players[0].buttons & 3);
                        expected.players[1].dpad =
                            (expected.players[1].dpad & !5) | (mirrored.players[1].dpad & 5);
                    }
                }
            }
            assert_eq!(
                session.selected_branch().input_at(frame as u64),
                expected,
                "{transform:?} frame {frame}"
            );
        }
        assert_eq!(snapshot(&mut state).ranges, [(1, 3), (6, 9)]);
        state.reduce(TasEditorAction::Undo).unwrap();
        assert_eq!(bytes(&state), before);
        assert!(snapshot(&mut state).ranges.is_empty());
        state.reduce(TasEditorAction::Redo).unwrap();
        assert_eq!(bytes(&state), after);
    }
}

#[test]
fn marked_ranges_normalize_limit_remove_clear_without_movie_history() {
    let (_root, mut state) = tests::state_with_project(200);
    let before = bytes(&state);
    let history = state.session.as_ref().unwrap().undo_count();
    for (start, end) in [(8, 10), (2, 4), (3, 8), (2, 10)] {
        add(&mut state, start, end).unwrap();
    }
    assert_eq!(snapshot(&mut state).ranges, [(2, 10)]);
    let clear = action(TasMarkedRangesAction::Clear {
        snapshot: snapshot(&mut state),
    });
    state.reduce(clear).unwrap();
    for index in 0..64 {
        add(&mut state, index * 3, index * 3 + 1).unwrap();
    }
    let full = snapshot(&mut state);
    assert!(add(&mut state, 195, 196).is_err());
    assert_eq!(snapshot(&mut state), full);
    // Joining existing ranges is permitted even when the list has reached its cap.
    add(&mut state, 0, 190).unwrap();
    assert_eq!(snapshot(&mut state).ranges, [(0, 190)]);
    let remove = action(TasMarkedRangesAction::Remove {
        snapshot: snapshot(&mut state),
        index: 0,
    });
    state.reduce(remove).unwrap();
    assert!(snapshot(&mut state).ranges.is_empty());
    assert_eq!(bytes(&state), before);
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history);
}

#[test]
fn marked_ranges_reject_stale_revision_selection_movie_and_forged_ranges() {
    let (_root, mut state) = tests::state_with_project(20);
    add(&mut state, 1, 3).unwrap();
    let stale = apply(&mut state, TasDigitalTransform::Invert);
    add(&mut state, 8, 10).unwrap();
    let before = bytes(&state);
    assert!(state.reduce(stale).is_err());
    let mut forged = snapshot(&mut state);
    forged.ranges = vec![(1, 10)];
    assert!(
        state
            .reduce(action(TasMarkedRangesAction::Clear { snapshot: forged }))
            .is_err()
    );
    let stale_selection = state
        .timeline_selection
        .snapshot(state.session.as_ref().unwrap())
        .unwrap();
    state
        .timeline_selection
        .select_range(state.session.as_ref().unwrap(), 12, 15);
    let stale_add = action(TasMarkedRangesAction::Add {
        snapshot: snapshot(&mut state),
        selection: stale_selection,
    });
    assert!(state.reduce(stale_add).is_err());
    let bad_movie = action(TasMarkedRangesAction::Apply {
        snapshot: snapshot(&mut state),
        target_movie_sha256: TasDigest([0xA5; 32]),
        mask: mask(),
        transform: TasDigitalTransform::Invert,
    });
    assert!(state.reduce(bad_movie).is_err());
    let bad_index = action(TasMarkedRangesAction::Remove {
        snapshot: snapshot(&mut state),
        index: 2,
    });
    assert!(state.reduce(bad_index).is_err());
    assert_eq!(bytes(&state), before);
    assert_eq!(snapshot(&mut state).ranges, [(1, 3), (8, 10)]);
}

#[test]
fn marked_ranges_clear_reset_cannot_resurrect_queued_actions() {
    let (_root, mut state) = tests::state_with_project(10);
    add(&mut state, 1, 3).unwrap();
    let stale = apply(&mut state, TasDigitalTransform::Invert);
    state.input_clipboard.clear().unwrap();
    add(&mut state, 1, 3).unwrap();
    let before = bytes(&state);
    assert!(state.reduce(stale).is_err());
    assert_eq!(bytes(&state), before);
}

#[test]
fn marked_ranges_unrelated_edit_invalidates_list_and_queued_apply() {
    let (_root, mut state) = tests::state_with_project(10);
    add(&mut state, 1, 3).unwrap();
    let stale = apply(&mut state, TasDigitalTransform::Invert);
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 9,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    assert!(
        state
            .input_clipboard
            .marked_ranges
            .visible_ranges(state.session.as_ref().unwrap())
            .is_empty()
    );
    let before = bytes(&state);
    assert!(state.reduce(stale).is_err());
    assert!(snapshot(&mut state).ranges.is_empty());
    assert_eq!(bytes(&state), before);
}

#[test]
fn marked_ranges_no_op_and_invalid_mask_leave_history_and_generation_unchanged() {
    let (_root, mut state) = tests::state_with_project(10);
    add(&mut state, 1, 3).unwrap();
    add(&mut state, 7, 9).unwrap();
    let before = bytes(&state);
    let history = state.session.as_ref().unwrap().undo_count();
    let marks = snapshot(&mut state);
    let edit = apply(&mut state, TasDigitalTransform::Clear);
    state.reduce(edit).unwrap();
    assert_eq!(snapshot(&mut state), marks);
    for mask in [TasDigitalInputMask::default(), {
        let mut mask = mask();
        mask.players[0].dpad = 0x80;
        mask
    }] {
        let session = state.session.as_ref().unwrap();
        let target_movie_sha256 = session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap();
        let edit = action(TasMarkedRangesAction::Apply {
            snapshot: snapshot(&mut state),
            target_movie_sha256,
            mask,
            transform: TasDigitalTransform::Invert,
        });
        assert!(state.reduce(edit).is_err());
    }
    assert_eq!(bytes(&state), before);
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history);
    assert_eq!(snapshot(&mut state), marks);
}

#[test]
fn marked_ranges_authoring_failure_in_late_range_is_atomic() {
    let (_root, mut state) = tests::state_with_project(10);
    let input = TasInputFrame {
        tilt_x_bits: 1.0_f32.to_bits(),
        ..TasInputFrame::default()
    };
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| edit.set_input_range("main", 7, 1, input))
        .unwrap();
    add(&mut state, 1, 3).unwrap();
    add(&mut state, 7, 9).unwrap();
    let before = bytes(&state);
    let history = state.session.as_ref().unwrap().undo_count();
    let edit = apply(&mut state, TasDigitalTransform::Invert);
    assert!(state.reduce(edit).is_err());
    assert_eq!(bytes(&state), before);
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history);
}

#[test]
fn marked_ranges_obey_live_authority() {
    let (_root, mut state) = tests::state_with_project(10);
    add(&mut state, 1, 3).unwrap();
    let edit = apply(&mut state, TasDigitalTransform::Invert);
    state.set_live_status(TasEditorLiveStatus::Staging {
        completed: 0,
        total: 1,
    });
    let before = bytes(&state);
    let error = state.reduce(edit).unwrap_err().to_string();
    assert!(error.contains("live game decision"), "{error}");
    assert_eq!(bytes(&state), before);
}

#[test]
fn marked_ranges_discard_the_recording_draft_before_editing_like_clipboard_actions() {
    let (_root, mut state) = tests::state_with_project(10);
    add(&mut state, 1, 3).unwrap();
    let edit = apply(&mut state, TasDigitalTransform::Invert);
    let before = bytes(&state);
    let history = state.session.as_ref().unwrap().undo_count();
    state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();
    assert!(state.recording.is_some());
    state.reduce(edit).unwrap();
    assert!(state.recording.is_none());
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 10);
    assert_eq!(session.undo_count(), history + 1);
    assert_eq!(session.selected_branch().input_at(1).players[0].buttons, 3);
    state.reduce(TasEditorAction::Undo).unwrap();
    assert_eq!(bytes(&state), before);
}

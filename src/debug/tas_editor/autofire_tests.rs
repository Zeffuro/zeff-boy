use std::{collections::BTreeMap, ops::Range};

use super::*;
use crate::tas_project::{TasControllerInput, TasInputFrame};

fn set_input(state: &mut TasEditorWindowState, start: u64, length: u64, input: TasInputFrame) {
    let session = state.session.as_mut().unwrap();
    let branch_id = session.selected_branch_id().to_owned();
    session
        .edit_transaction(move |edit| edit.set_input_range(&branch_id, start, length, input))
        .unwrap();
}

#[derive(Clone, Copy)]
struct AutofireConfig {
    player: usize,
    buttons_mask: u8,
    dpad_mask: u8,
    period: u8,
    on_frames: u8,
}

fn autofire_action(
    state: &TasEditorWindowState,
    range: Range<u64>,
    config: AutofireConfig,
) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    TasEditorAction::InputClipboard(
        input_clipboard::TasInputClipboardAction::ApplyDigitalAutofire(
            input_clipboard::TasDigitalAutofireAction {
                expected_project_sha256: session.project_content_sha256(),
                target_branch_id: session.selected_branch_id().to_owned(),
                target_movie_sha256: session
                    .project()
                    .branch_movie_sha256(session.selected_branch_id())
                    .unwrap(),
                selection: timeline_selection::TasInputSelection {
                    branch_id: session.selected_branch_id().to_owned(),
                    start: range.start,
                    end: range.end,
                },
                player: config.player,
                buttons_mask: config.buttons_mask,
                dpad_mask: config.dpad_mask,
                period: config.period,
                on_frames: config.on_frames,
            },
        ),
    )
}

const BUTTON_AUTOFIRE: AutofireConfig = AutofireConfig {
    player: 0,
    buttons_mask: 1,
    dpad_mask: 0,
    period: 2,
    on_frames: 1,
};

fn select_range(state: &mut TasEditorWindowState, start: u64, end: u64) {
    let session = state.session.as_ref().unwrap();
    state.timeline_selection.select_range(session, start, end);
}

#[test]
fn digital_autofire_preserves_other_players_axes_and_special_input() {
    let mut input = TasInputFrame::default();
    input.players[0] = TasControllerInput {
        buttons: 0b0000_0010,
        dpad: 0b0000_0100,
    };
    input.players[1] = TasControllerInput {
        buttons: 0b1010_0000,
        dpad: 0b0000_1000,
    };
    input.tilt_x_bits = 0x7FC0_0123;
    input.tilt_y_bits = 0xBF00_0000;
    let (_root, mut state) = special_input_tests::synthetic_state(
        "gba",
        &[special_input_editor::GBA_TILT_DEVICE],
        input,
        BTreeMap::new(),
    );
    set_input(&mut state, 0, 3, input);
    select_range(&mut state, 0, 3);

    state
        .reduce(autofire_action(&state, 0..3, BUTTON_AUTOFIRE))
        .unwrap();

    let branch = state.session.as_ref().unwrap().selected_branch();
    for (frame, pressed) in [(0, true), (1, false), (2, true)] {
        let edited = branch.input_at(frame);
        assert_eq!(edited.players[0].buttons & !1, input.players[0].buttons);
        assert_eq!(edited.players[0].dpad, input.players[0].dpad);
        assert_eq!(edited.players[1], input.players[1]);
        assert_eq!(
            (edited.tilt_x_bits, edited.tilt_y_bits),
            (input.tilt_x_bits, input.tilt_y_bits)
        );
        assert_eq!(edited.players[0].buttons & 1 != 0, pressed);
    }
}

#[test]
fn digital_autofire_leaves_replay_events_untouched_and_is_one_undo_step() {
    use zeff_emu_common::replay::ReplayEvent;

    let events = vec![ReplayEvent::FdsDiskSide { frame: 2, side: 1 }];
    let (_root, mut state) = event_tests::fds_state(6, events.clone());
    let input = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0b0000_0010,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };
    set_input(&mut state, 1, 4, input);
    select_range(&mut state, 1, 5);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    let before_undo = state.session.as_ref().unwrap().undo_count();

    state
        .reduce(autofire_action(
            &state,
            1..5,
            AutofireConfig {
                period: 3,
                on_frames: 2,
                ..BUTTON_AUTOFIRE
            },
        ))
        .unwrap();

    let after = state.session.as_ref().unwrap().project().encode().unwrap();
    assert_ne!(after, before);
    assert_eq!(
        state.session.as_ref().unwrap().selected_branch().events(),
        events
    );
    assert_eq!(
        state.session.as_ref().unwrap().undo_count(),
        before_undo + 1
    );
    state.reduce(TasEditorAction::Undo).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
    state.reduce(TasEditorAction::Redo).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        after
    );
}

#[test]
fn digital_autofire_rejects_stale_selection_and_live_authority_without_mutation() {
    let (_root, mut state) = tests::state_with_project(5);
    select_range(&mut state, 0, 3);
    let stale = autofire_action(&state, 0..3, BUTTON_AUTOFIRE);
    select_range(&mut state, 1, 4);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(stale).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    select_range(&mut state, 1, 4);
    let locked = autofire_action(&state, 1..4, BUTTON_AUTOFIRE);
    state.set_live_status(TasEditorLiveStatus::Staging {
        completed: 0,
        total: 1,
    });
    assert!(state.reduce(locked).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

#[test]
fn digital_autofire_rejects_invalid_bounds_coleco_and_no_ops_without_history() {
    let (_root, mut state) = tests::state_with_project(4);
    select_range(&mut state, 0, 4);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    let history = state.session.as_ref().unwrap().undo_count();
    for action in [
        autofire_action(
            &state,
            0..4,
            AutofireConfig {
                buttons_mask: 0,
                dpad_mask: 0,
                ..BUTTON_AUTOFIRE
            },
        ),
        autofire_action(
            &state,
            0..4,
            AutofireConfig {
                period: 0,
                on_frames: 0,
                ..BUTTON_AUTOFIRE
            },
        ),
        autofire_action(
            &state,
            0..4,
            AutofireConfig {
                period: 61,
                ..BUTTON_AUTOFIRE
            },
        ),
        autofire_action(
            &state,
            0..4,
            AutofireConfig {
                on_frames: 3,
                ..BUTTON_AUTOFIRE
            },
        ),
    ] {
        assert!(state.reduce(action).is_err());
    }
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history);

    let no_op = autofire_action(
        &state,
        0..4,
        AutofireConfig {
            on_frames: 0,
            ..BUTTON_AUTOFIRE
        },
    );
    assert_eq!(
        state.reduce(no_op).unwrap(),
        Some("Digital autofire made no change".to_owned())
    );
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history);

    let (_root, mut coleco) = special_input_tests::synthetic_state(
        "coleco",
        &["standard-controller"],
        TasInputFrame::default(),
        BTreeMap::new(),
    );
    select_range(&mut coleco, 0, 3);
    let before = coleco.session.as_ref().unwrap().project().encode().unwrap();
    assert!(
        coleco
            .reduce(autofire_action(&coleco, 0..3, BUTTON_AUTOFIRE))
            .is_err()
    );
    assert_eq!(
        coleco.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

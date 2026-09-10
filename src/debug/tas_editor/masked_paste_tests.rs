use std::collections::BTreeMap;

use super::*;
use crate::tas_project::{
    TasControllerInput, TasDeviceIdentity, TasDigest, TasDigitalInputMask, TasExternalIdentity,
    TasInitialBranch, TasInputFrame, TasProject, TasProjectIdentity,
};
use zeff_emu_common::replay::{ReplayEvent, ReplayStartMetadata};

fn set_input(state: &mut TasEditorWindowState, frame: u64, input: TasInputFrame) {
    let session = state.session.as_mut().unwrap();
    let branch_id = session.selected_branch_id().to_owned();
    session
        .edit_transaction(move |edit| edit.set_input_range(&branch_id, frame, 1, input))
        .unwrap();
}

fn select_range(state: &mut TasEditorWindowState, start: u64, end: u64) {
    let session = state.session.as_ref().unwrap();
    state.timeline_selection.select_range(session, start, end);
}

fn copy_selection(state: &mut TasEditorWindowState, start: u64, end: u64) {
    select_range(state, start, end);
    let session = state.session.as_ref().unwrap();
    let branch = session.selected_branch();
    let events = branch
        .events()
        .iter()
        .filter(|event| (start..end).contains(&event.frame()))
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

fn mask(player_zero: TasControllerInput, player_one: TasControllerInput) -> TasDigitalInputMask {
    let mut mask = TasDigitalInputMask::default();
    mask.players[0] = player_zero;
    mask.players[1] = player_one;
    mask
}

fn masked_paste_action(
    state: &TasEditorWindowState,
    mask: TasDigitalInputMask,
    destination: input_clipboard::TasMaskedPasteDestination,
) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    TasEditorAction::InputClipboard(
        input_clipboard::TasInputClipboardAction::PasteMaskedControls(
            input_clipboard::TasMaskedPasteAction {
                expected_project_sha256: session.project_content_sha256(),
                target_branch_id: session.selected_branch_id().to_owned(),
                target_movie_sha256: session
                    .project()
                    .branch_movie_sha256(session.selected_branch_id())
                    .unwrap(),
                clipboard_generation: state.input_clipboard.generation(),
                mask,
                destination,
            },
        ),
    )
}

fn gba_two_player_tilt_state() -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let root = crate::test_support::test_directory("tas-editor-masked-paste-gba").unwrap();
    let start_state = vec![0xA5; 16];
    let project = TasProject::new(
        "masked-paste-gba",
        TasProjectIdentity {
            system: "gba".to_owned(),
            core_family: "masked-paste-test".to_owned(),
            determinism_abi: "masked-paste-test-v1".to_owned(),
            source_media_sha256: TasDigest([1; 32]),
            effective_media_sha256: TasDigest([2; 32]),
            patches: Vec::new(),
            firmware: Vec::new(),
            devices: vec![
                TasDeviceIdentity {
                    port: "p1".to_owned(),
                    device: "gamepad".to_owned(),
                    configuration_sha256: TasDigest([3; 32]),
                },
                TasDeviceIdentity {
                    port: "p2".to_owned(),
                    device: "gamepad".to_owned(),
                    configuration_sha256: TasDigest([4; 32]),
                },
                TasDeviceIdentity {
                    port: "tilt".to_owned(),
                    device: special_input_editor::GBA_TILT_DEVICE.to_owned(),
                    configuration_sha256: TasDigest([5; 32]),
                },
            ],
            sync_config_sha256: TasDigest([6; 32]),
            persistent_state: TasExternalIdentity::Absent,
            rtc_state: TasExternalIdentity::Absent,
            sensor_state: TasExternalIdentity::Absent,
            cheats: TasExternalIdentity::Absent,
            state_format_compatibility_id: "masked-paste-gba-v1".to_owned(),
            start_state_sha256: TasDigest::from_bytes(&start_state),
        },
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 6,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )
    .unwrap();
    let manual = root.path().join("movie.ztas");
    project.save_atomic(&manual).unwrap();
    let mut state = TasEditorWindowState::with_seek_cache_root(root.path().join("seek-cache"));
    state.reduce(TasEditorAction::OpenProject(manual)).unwrap();
    (root, state)
}

#[test]
fn cursor_masked_paste_copies_checked_players_clears_neutral_source_and_undoes() {
    let (_root, mut state) = gba_two_player_tilt_state();
    let source = TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 1,
                dpad: 0,
            },
            TasControllerInput {
                buttons: 0,
                dpad: 4,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ],
        ..TasInputFrame::default()
    };
    set_input(&mut state, 0, source);
    let destination = [
        TasInputFrame {
            players: [
                TasControllerInput {
                    buttons: 0b10_0001,
                    dpad: 8,
                },
                TasControllerInput {
                    buttons: 0b10,
                    dpad: 12,
                },
                TasControllerInput {
                    buttons: 0b10_0000,
                    dpad: 2,
                },
                TasControllerInput::default(),
                TasControllerInput::default(),
            ],
            tilt_x_bits: 0.25f32.to_bits(),
            tilt_y_bits: (-0.5f32).to_bits(),
            ..TasInputFrame::default()
        },
        TasInputFrame {
            players: [
                TasControllerInput {
                    buttons: 0b10_0001,
                    dpad: 1,
                },
                TasControllerInput {
                    buttons: 0b10,
                    dpad: 12,
                },
                TasControllerInput {
                    buttons: 0b1_0000,
                    dpad: 1,
                },
                TasControllerInput::default(),
                TasControllerInput::default(),
            ],
            tilt_x_bits: (-0.75f32).to_bits(),
            tilt_y_bits: 0.125f32.to_bits(),
            ..TasInputFrame::default()
        },
    ];
    for (offset, input) in destination.into_iter().enumerate() {
        set_input(&mut state, offset as u64 + 3, input);
    }
    copy_selection(&mut state, 0, 2);
    state.reduce(TasEditorAction::SelectCursor(3)).unwrap();
    let mask = mask(
        TasControllerInput {
            buttons: 1,
            dpad: 0,
        },
        TasControllerInput {
            buttons: 0,
            dpad: 4,
        },
    );
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    let history = state.session.as_ref().unwrap().undo_count();

    assert_eq!(
        state
            .reduce(masked_paste_action(
                &state,
                mask,
                input_clipboard::TasMaskedPasteDestination::Cursor(3),
            ))
            .unwrap(),
        Some("Pasted checked controls".to_owned())
    );
    let branch = state.session.as_ref().unwrap().selected_branch();
    let first = branch.input_at(3);
    assert_eq!(first.players[0].buttons, 0b10_0001);
    assert_eq!(first.players[1].dpad, 12);
    assert_eq!(first.players[2], destination[0].players[2]);
    assert_eq!(
        (first.tilt_x_bits, first.tilt_y_bits),
        (destination[0].tilt_x_bits, destination[0].tilt_y_bits)
    );
    let second = branch.input_at(4);
    assert_eq!(second.players[0].buttons, 0b10_0000);
    assert_eq!(second.players[1].dpad, 8);
    assert_eq!(second.players[2], destination[1].players[2]);
    assert_eq!(
        (second.tilt_x_bits, second.tilt_y_bits),
        (destination[1].tilt_x_bits, destination[1].tilt_y_bits)
    );
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history + 1);
    let after = state.session.as_ref().unwrap().project().encode().unwrap();
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
fn selection_masked_paste_tiles_without_changing_copied_or_destination_events() {
    let events = vec![
        ReplayEvent::FdsDiskSide { frame: 1, side: 1 },
        ReplayEvent::FdsDiskSide { frame: 4, side: 2 },
        ReplayEvent::FdsDiskSide { frame: 6, side: 3 },
    ];
    let (_root, mut state) = event_tests::fds_state(8, events.clone());
    for (frame, buttons) in [(0, 1), (1, 0), (2, 1)] {
        set_input(
            &mut state,
            frame,
            TasInputFrame {
                players: [TasControllerInput { buttons, dpad: 0 }; 5],
                ..TasInputFrame::default()
            },
        );
    }
    for frame in 3..8 {
        set_input(
            &mut state,
            frame,
            TasInputFrame {
                players: [TasControllerInput {
                    buttons: 0b10,
                    dpad: frame as u8,
                }; 5],
                ..TasInputFrame::default()
            },
        );
    }
    copy_selection(&mut state, 0, 3);
    select_range(&mut state, 3, 8);
    let mask = mask(
        TasControllerInput {
            buttons: 1,
            dpad: 0,
        },
        TasControllerInput::default(),
    );

    assert_eq!(
        state
            .reduce(masked_paste_action(
                &state,
                mask,
                input_clipboard::TasMaskedPasteDestination::Selection(
                    timeline_selection::TasInputSelection {
                        branch_id: "main".to_owned(),
                        start: 3,
                        end: 8,
                    },
                ),
            ))
            .unwrap(),
        Some("Tiled checked controls".to_owned())
    );
    let branch = state.session.as_ref().unwrap().selected_branch();
    for (frame, pressed) in [(3, true), (4, false), (5, true), (6, true), (7, false)] {
        let input = branch.input_at(frame);
        assert_eq!(input.players[0].buttons & 1 != 0, pressed);
        assert_eq!(input.players[0].buttons & !1, 0b10);
        assert_eq!(input.players[0].dpad, frame as u8);
    }
    assert_eq!(branch.events(), events);
}

#[test]
fn masked_paste_rejects_stale_invalid_no_op_and_locked_requests_atomically() {
    let (_root, mut state) = tests::state_with_project(5);
    let source = TasInputFrame {
        players: [TasControllerInput {
            buttons: 1,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };
    set_input(&mut state, 0, source);
    set_input(&mut state, 2, source);
    copy_selection(&mut state, 0, 1);
    state.reduce(TasEditorAction::SelectCursor(2)).unwrap();
    let valid_mask = mask(
        TasControllerInput {
            buttons: 1,
            dpad: 0,
        },
        TasControllerInput::default(),
    );

    let stale_generation = masked_paste_action(
        &state,
        valid_mask,
        input_clipboard::TasMaskedPasteDestination::Cursor(2),
    );
    copy_selection(&mut state, 0, 1);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(stale_generation).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    state.reduce(TasEditorAction::SelectCursor(2)).unwrap();
    let stale_cursor = masked_paste_action(
        &state,
        valid_mask,
        input_clipboard::TasMaskedPasteDestination::Cursor(2),
    );
    state.reduce(TasEditorAction::SelectCursor(3)).unwrap();
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(stale_cursor).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    select_range(&mut state, 1, 4);
    let stale_selection = masked_paste_action(
        &state,
        valid_mask,
        input_clipboard::TasMaskedPasteDestination::Selection(
            timeline_selection::TasInputSelection {
                branch_id: "main".to_owned(),
                start: 1,
                end: 4,
            },
        ),
    );
    select_range(&mut state, 2, 5);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(stale_selection).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    state.reduce(TasEditorAction::SelectCursor(2)).unwrap();
    let history = state.session.as_ref().unwrap().undo_count();
    assert_eq!(
        state
            .reduce(masked_paste_action(
                &state,
                valid_mask,
                input_clipboard::TasMaskedPasteDestination::Cursor(2),
            ))
            .unwrap(),
        Some("Control paste made no change".to_owned())
    );
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history);
    for invalid_mask in [
        TasDigitalInputMask::default(),
        mask(
            TasControllerInput {
                buttons: 0,
                dpad: 0b1_0000,
            },
            TasControllerInput::default(),
        ),
    ] {
        let before = state.session.as_ref().unwrap().project().encode().unwrap();
        assert!(
            state
                .reduce(masked_paste_action(
                    &state,
                    invalid_mask,
                    input_clipboard::TasMaskedPasteDestination::Cursor(2),
                ))
                .is_err()
        );
        assert_eq!(
            state.session.as_ref().unwrap().project().encode().unwrap(),
            before
        );
    }

    let locked = masked_paste_action(
        &state,
        valid_mask,
        input_clipboard::TasMaskedPasteDestination::Cursor(2),
    );
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    state.set_live_status(TasEditorLiveStatus::Staging {
        completed: 0,
        total: 1,
    });
    assert!(state.reduce(locked).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    let (_root, mut coleco) = special_input_tests::synthetic_state(
        "coleco",
        &["standard-controller"],
        TasInputFrame::default(),
        BTreeMap::new(),
    );
    let before = coleco.session.as_ref().unwrap().project().encode().unwrap();
    assert!(
        coleco
            .reduce(masked_paste_action(
                &coleco,
                valid_mask,
                input_clipboard::TasMaskedPasteDestination::Cursor(0),
            ))
            .is_err()
    );
    assert_eq!(
        coleco.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

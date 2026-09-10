use std::collections::BTreeMap;

use super::*;
use crate::tas_project::{
    TasControllerInput, TasDeviceIdentity, TasDigest, TasDigitalInputMask, TasDigitalTransform,
    TasExternalIdentity, TasFirmwareIdentity, TasInitialBranch, TasInputFrame, TasProject,
    TasProjectIdentity,
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

fn transform_action(
    state: &TasEditorWindowState,
    start: u64,
    end: u64,
    mask: TasDigitalInputMask,
    transform: TasDigitalTransform,
) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    TasEditorAction::InputClipboard(
        input_clipboard::TasInputClipboardAction::ApplyDigitalTransform(
            input_clipboard::TasDigitalTransformAction {
                expected_project_sha256: session.project_content_sha256(),
                target_branch_id: session.selected_branch_id().to_owned(),
                target_movie_sha256: session
                    .project()
                    .branch_movie_sha256(session.selected_branch_id())
                    .unwrap(),
                selection: timeline_selection::TasInputSelection {
                    branch_id: session.selected_branch_id().to_owned(),
                    start,
                    end,
                },
                mask,
                transform,
            },
        ),
    )
}

fn mask(player_zero: TasControllerInput, player_one: TasControllerInput) -> TasDigitalInputMask {
    let mut mask = TasDigitalInputMask::default();
    mask.players[0] = player_zero;
    mask.players[1] = player_one;
    mask
}

pub(super) fn fds_two_player_state(
    frame_count: u64,
    events: Vec<ReplayEvent>,
) -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let root = crate::test_support::test_directory("tas-editor-fds-digital-transform").unwrap();
    let start_state = vec![0xD5; 16];
    let project = TasProject::new(
        "ui-fds-digital-transform",
        TasProjectIdentity {
            system: "nes".to_owned(),
            core_family: "nes-test".to_owned(),
            determinism_abi: "nes-test-sync-v1".to_owned(),
            source_media_sha256: TasDigest([1; 32]),
            effective_media_sha256: TasDigest([1; 32]),
            patches: Vec::new(),
            firmware: vec![TasFirmwareIdentity::External {
                firmware_id: "nintendo.fds.bios".to_owned(),
                variant: Some("test".to_owned()),
                sha256: TasDigest([2; 32]),
            }],
            devices: (1..=2)
                .map(|player| TasDeviceIdentity {
                    port: format!("p{player}"),
                    device: "nes-standard-controller".to_owned(),
                    configuration_sha256: TasDigest([player as u8 + 3; 32]),
                })
                .collect(),
            sync_config_sha256: TasDigest([3; 32]),
            persistent_state: TasExternalIdentity::Absent,
            rtc_state: TasExternalIdentity::Absent,
            sensor_state: TasExternalIdentity::Absent,
            cheats: TasExternalIdentity::Absent,
            state_format_compatibility_id: "nes-fds-test-state-v1".to_owned(),
            start_state_sha256: TasDigest::from_bytes(&start_state),
        },
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count,
            input_spans: Vec::new(),
            events,
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

fn varied_input(index: u8) -> TasInputFrame {
    TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 0b1100 | index,
                dpad: 0b1000 | index,
            },
            TasControllerInput {
                buttons: 0b1000 | index,
                dpad: 0b1010 | index,
            },
            TasControllerInput {
                buttons: index,
                dpad: index.rotate_left(1),
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
        ],
        ..TasInputFrame::default()
    }
}

#[test]
fn reverse_moves_only_the_selected_controls_and_preserves_fds_events() {
    let events = vec![
        ReplayEvent::FdsDiskSide { frame: 2, side: 1 },
        ReplayEvent::FdsDiskSide { frame: 5, side: 2 },
    ];
    let (_root, mut state) = fds_two_player_state(6, events.clone());
    for index in 0..4 {
        set_input(&mut state, index + 1, varied_input(index as u8));
    }
    let before = (1..5)
        .map(|frame| {
            state
                .session
                .as_ref()
                .unwrap()
                .selected_branch()
                .input_at(frame)
        })
        .collect::<Vec<_>>();
    let mask = mask(
        TasControllerInput {
            buttons: 0b0011,
            dpad: 0,
        },
        TasControllerInput {
            buttons: 0,
            dpad: 0b0101,
        },
    );
    select_range(&mut state, 1, 5);
    let history = state.session.as_ref().unwrap().undo_count();

    state
        .reduce(transform_action(
            &state,
            1,
            5,
            mask,
            TasDigitalTransform::Reverse,
        ))
        .unwrap();

    let branch = state.session.as_ref().unwrap().selected_branch();
    for (offset, source) in before.iter().copied().enumerate() {
        let mirrored = before[before.len() - 1 - offset];
        let mut expected = source;
        expected.players[0].buttons =
            (source.players[0].buttons & !0b0011) | (mirrored.players[0].buttons & 0b0011);
        expected.players[1].dpad =
            (source.players[1].dpad & !0b0101) | (mirrored.players[1].dpad & 0b0101);
        assert_eq!(branch.input_at(offset as u64 + 1), expected);
    }
    assert_eq!(branch.events(), events);
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history + 1);
}

#[test]
fn invert_is_undoable_in_one_transaction() {
    let (_root, mut state) = tests::state_with_project(4);
    let inputs = [
        TasInputFrame {
            players: [TasControllerInput {
                buttons: 0b1010,
                dpad: 0b1001,
            }; 5],
            ..TasInputFrame::default()
        },
        TasInputFrame {
            players: [TasControllerInput {
                buttons: 0b0111,
                dpad: 0b0110,
            }; 5],
            ..TasInputFrame::default()
        },
    ];
    for (offset, input) in inputs.into_iter().enumerate() {
        set_input(&mut state, offset as u64 + 1, input);
    }
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    let history = state.session.as_ref().unwrap().undo_count();
    let mask = mask(
        TasControllerInput {
            buttons: 0b0101,
            dpad: 0b0011,
        },
        TasControllerInput::default(),
    );
    select_range(&mut state, 1, 3);

    state
        .reduce(transform_action(
            &state,
            1,
            3,
            mask,
            TasDigitalTransform::Invert,
        ))
        .unwrap();
    let after = state.session.as_ref().unwrap().project().encode().unwrap();
    assert_ne!(after, before);
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history + 1);
    for (offset, input) in inputs.into_iter().enumerate() {
        let edited = state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(offset as u64 + 1);
        assert_eq!(edited.players[0].buttons, input.players[0].buttons ^ 0b0101);
        assert_eq!(edited.players[0].dpad, input.players[0].dpad ^ 0b0011);
        assert_eq!(edited.players[1..], input.players[1..]);
    }
    state.reduce(TasEditorAction::Undo).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

#[test]
fn clear_changes_only_its_mask_once_and_repeated_clear_is_a_no_op() {
    let (_root, mut state) = tests::state_with_project(4);
    let source = TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 0b1011,
                dpad: 0b0110,
            },
            TasControllerInput {
                buttons: 0b0101,
                dpad: 0b1001,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ],
        ..TasInputFrame::default()
    };
    set_input(&mut state, 1, source);
    set_input(&mut state, 2, source);
    let mask = mask(
        TasControllerInput {
            buttons: 0b0011,
            dpad: 0,
        },
        TasControllerInput::default(),
    );
    select_range(&mut state, 1, 3);
    let history = state.session.as_ref().unwrap().undo_count();

    assert_eq!(
        state
            .reduce(transform_action(
                &state,
                1,
                3,
                mask,
                TasDigitalTransform::Clear,
            ))
            .unwrap(),
        Some("Cleared selected controls".to_owned())
    );
    for frame in 1..3 {
        let edited = state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(frame);
        assert_eq!(edited.players[0].buttons, 0b1000);
        assert_eq!(edited.players[0].dpad, source.players[0].dpad);
        assert_eq!(edited.players[1], source.players[1]);
    }
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history + 1);
    assert_eq!(
        state
            .reduce(transform_action(
                &state,
                1,
                3,
                mask,
                TasDigitalTransform::Clear,
            ))
            .unwrap(),
        Some("Control edit made no change".to_owned())
    );
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history + 1);

    let locked = transform_action(&state, 1, 3, mask, TasDigitalTransform::Invert);
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
}

#[test]
fn transform_rejects_stale_invalid_and_coleco_requests_without_mutation() {
    let (_root, mut state) = tests::state_with_project(4);
    let valid_mask = mask(
        TasControllerInput {
            buttons: 1,
            dpad: 0,
        },
        TasControllerInput::default(),
    );
    select_range(&mut state, 0, 3);
    let stale_selection = transform_action(&state, 0, 3, valid_mask, TasDigitalTransform::Clear);
    select_range(&mut state, 1, 4);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(stale_selection).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    select_range(&mut state, 0, 3);
    let stale_project = transform_action(&state, 0, 3, valid_mask, TasDigitalTransform::Clear);
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 0,
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

    select_range(&mut state, 0, 3);
    for invalid_mask in [
        TasDigitalInputMask::default(),
        mask(
            TasControllerInput {
                buttons: 0,
                dpad: 0b0001_0000,
            },
            TasControllerInput::default(),
        ),
        mask(
            TasControllerInput {
                buttons: 0,
                dpad: 0b1000_0000,
            },
            TasControllerInput {
                buttons: 0,
                dpad: 0,
            },
        ),
    ] {
        let before = state.session.as_ref().unwrap().project().encode().unwrap();
        assert!(
            state
                .reduce(transform_action(
                    &state,
                    0,
                    3,
                    invalid_mask,
                    TasDigitalTransform::Clear,
                ))
                .is_err()
        );
        assert_eq!(
            state.session.as_ref().unwrap().project().encode().unwrap(),
            before
        );
    }

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
            .reduce(transform_action(
                &coleco,
                0,
                3,
                valid_mask,
                TasDigitalTransform::Clear,
            ))
            .is_err()
    );
    assert_eq!(
        coleco.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

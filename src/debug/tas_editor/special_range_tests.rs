use std::collections::BTreeMap;

use super::*;
use crate::tas_project::{
    TasCameraInput, TasDigest, TasInputFrame, TasSpecialInputMask, TasSpecialTransform,
    TasZapperInput,
};
use input_clipboard::marked_ranges::TasMarkedRangesAction;
use input_clipboard::special_ranges::{TasSpecialRangeAction, TasSpecialRangeTarget};
use special_input_editor::{
    GAME_BOY_MBC7_DEVICE, GAME_BOY_POCKET_CAMERA_DEVICE, GBA_TILT_DEVICE, NES_ZAPPER_DEVICE,
    special_input_capabilities,
};
use zeff_emu_common::replay::POCKET_CAMERA_FRAME_BYTES;

#[path = "special_range_execution_tests.rs"]
mod execution;

fn select(state: &mut TasEditorWindowState, start: u64, end: u64) {
    state
        .timeline_selection
        .select_range(state.session.as_ref().unwrap(), start, end);
}

fn mark(state: &mut TasEditorWindowState, start: u64, end: u64) {
    select(state, start, end);
    let session = state.session.as_ref().unwrap();
    let selection = state.timeline_selection.snapshot(session).unwrap();
    let snapshot = state
        .input_clipboard
        .marked_ranges
        .snapshot(session)
        .unwrap();
    state
        .reduce(TasEditorAction::InputClipboard(
            input_clipboard::TasInputClipboardAction::MarkedRanges(TasMarkedRangesAction::Add {
                snapshot,
                selection,
            }),
        ))
        .unwrap();
}

fn edit_action(
    state: &mut TasEditorWindowState,
    marked: bool,
    mask: TasSpecialInputMask,
    transform: TasSpecialTransform,
) -> TasEditorAction {
    let session = state.session.as_ref().unwrap();
    let target = if marked {
        TasSpecialRangeTarget::Marked(
            state
                .input_clipboard
                .marked_ranges
                .snapshot(session)
                .unwrap(),
        )
    } else {
        TasSpecialRangeTarget::Selection(state.timeline_selection.snapshot(session).unwrap())
    };
    wrap(TasSpecialRangeAction {
        expected_project_sha256: session.project_content_sha256(),
        target_branch_id: session.selected_branch_id().to_owned(),
        target_movie_sha256: session
            .project()
            .branch_movie_sha256(session.selected_branch_id())
            .unwrap(),
        target,
        mask,
        transform,
    })
}

fn wrap(action: TasSpecialRangeAction) -> TasEditorAction {
    TasEditorAction::InputClipboard(
        input_clipboard::TasInputClipboardAction::ApplySpecialTransform(action),
    )
}

fn bytes(state: &TasEditorWindowState) -> Vec<u8> {
    state.session.as_ref().unwrap().project().encode().unwrap()
}

fn fixture(
    system: &str,
    devices: &[&str],
    camera: bool,
) -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let assets = if camera {
        [
            vec![0x23; POCKET_CAMERA_FRAME_BYTES],
            vec![0x91; POCKET_CAMERA_FRAME_BYTES],
        ]
        .into_iter()
        .map(|data| (TasDigest::from_bytes(&data), data))
        .collect()
    } else {
        BTreeMap::new()
    };
    let (directory, mut state) =
        special_input_tests::synthetic_state(system, devices, TasInputFrame::default(), assets);
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| edit.insert_frames("main", 3, 9))
        .unwrap();
    let camera_assets = state
        .session
        .as_ref()
        .unwrap()
        .project()
        .assets()
        .keys()
        .copied()
        .collect::<Vec<_>>();
    let capabilities =
        special_input_capabilities(state.session.as_ref().unwrap().project().identity());
    for frame in 0..12 {
        let mut input = TasInputFrame::default();
        input.players[0].buttons = frame as u8;
        input.players[1].dpad = frame as u8 % 16;
        if capabilities.nes_zapper {
            input.zapper = TasZapperInput {
                enabled: true,
                trigger: frame % 2 == 0,
                hit: frame % 3 == 0,
                screen_pos: Some([frame as u16 * 10, frame as u16 * 7]),
            };
        }
        if capabilities.mbc7_tilt || capabilities.gba_tilt {
            input.tilt_x_bits = if frame == 2 {
                0x8000_0000
            } else {
                (frame as f32 / 12.0).to_bits()
            };
            input.tilt_y_bits = if frame == 3 {
                0x7FC0_0123
            } else {
                (-0.5 + frame as f32 / 24.0).to_bits()
            };
        }
        if capabilities.pocket_camera && frame % 3 != 0 {
            input.camera =
                TasCameraInput::Blob(camera_assets[frame as usize % camera_assets.len()]);
        }
        state
            .session
            .as_mut()
            .unwrap()
            .edit_transaction(|edit| edit.set_input_range("main", frame, 1, input))
            .unwrap();
    }
    (directory, state)
}

#[test]
fn special_ranges_clear_and_reverse_only_checked_channels_in_one_undoable_edit() {
    for (system, devices, mask) in [
        (
            "nes",
            vec![NES_ZAPPER_DEVICE],
            TasSpecialInputMask {
                zapper: true,
                ..Default::default()
            },
        ),
        (
            "gb",
            vec![GAME_BOY_MBC7_DEVICE, GAME_BOY_POCKET_CAMERA_DEVICE],
            TasSpecialInputMask {
                tilt_x: true,
                ..Default::default()
            },
        ),
        (
            "gb",
            vec![GAME_BOY_MBC7_DEVICE, GAME_BOY_POCKET_CAMERA_DEVICE],
            TasSpecialInputMask {
                tilt_y: true,
                camera: true,
                ..Default::default()
            },
        ),
        (
            "gba",
            vec![GBA_TILT_DEVICE],
            TasSpecialInputMask {
                tilt_x: true,
                tilt_y: true,
                ..Default::default()
            },
        ),
    ] {
        for transform in [TasSpecialTransform::Clear, TasSpecialTransform::Reverse] {
            for marked in [false, true] {
                let (_directory, mut state) = fixture(
                    system,
                    &devices,
                    devices.contains(&GAME_BOY_POCKET_CAMERA_DEVICE),
                );
                let ranges = if marked {
                    vec![(1_u64, 4_u64), (7, 10)]
                } else {
                    vec![(1, 10)]
                };
                for &(start, end) in &ranges {
                    mark(&mut state, start, end);
                }
                let before = bytes(&state);
                let session = state.session.as_ref().unwrap();
                let history = session.undo_count();
                let original = (0..12)
                    .map(|frame| session.selected_branch().input_at(frame))
                    .collect::<Vec<_>>();
                let selection = state.timeline_selection.snapshot(session);
                let assets = session.project().assets().clone();
                let action = edit_action(&mut state, marked, mask, transform);
                state.reduce(action).unwrap();
                let after = bytes(&state);
                let session = state.session.as_ref().unwrap();
                assert_eq!(session.undo_count(), history + 1);
                assert_eq!(state.timeline_selection.snapshot(session), selection);
                assert_eq!(session.project().assets(), &assets);
                for frame in 0..12 {
                    let mut expected = original[frame as usize];
                    if let Some(&(start, end)) = ranges
                        .iter()
                        .find(|&&(start, end)| frame >= start && frame < end)
                    {
                        let source = match transform {
                            TasSpecialTransform::Clear => TasInputFrame::default(),
                            TasSpecialTransform::Reverse => {
                                original[(start + end - 1 - frame) as usize]
                            }
                        };
                        if mask.zapper {
                            expected.zapper = source.zapper;
                        }
                        if mask.tilt_x {
                            expected.tilt_x_bits = source.tilt_x_bits;
                        }
                        if mask.tilt_y {
                            expected.tilt_y_bits = source.tilt_y_bits;
                        }
                        if mask.camera {
                            expected.camera = source.camera;
                        }
                    }
                    assert_eq!(
                        session.selected_branch().input_at(frame),
                        expected,
                        "{system} {transform:?} marked={marked} frame={frame}"
                    );
                }
                let marks = state
                    .input_clipboard
                    .marked_ranges
                    .snapshot(session)
                    .unwrap();
                assert_eq!(marks.ranges, if marked { ranges } else { Vec::new() });
                state.reduce(TasEditorAction::Undo).unwrap();
                assert_eq!(bytes(&state), before);
                state.reduce(TasEditorAction::Redo).unwrap();
                assert_eq!(bytes(&state), after);
            }
        }
    }
}

#[test]
fn special_ranges_reject_stale_selection_marks_movie_and_branch_without_mutation() {
    let (_directory, mut state) = fixture("gba", &[GBA_TILT_DEVICE], false);
    let mask = TasSpecialInputMask {
        tilt_x: true,
        ..Default::default()
    };
    select(&mut state, 1, 4);
    let stale_selection = edit_action(&mut state, false, mask, TasSpecialTransform::Clear);
    select(&mut state, 2, 5);
    assert!(state.reduce(stale_selection).is_err());
    mark(&mut state, 1, 4);
    let stale_marks = edit_action(&mut state, true, mask, TasSpecialTransform::Clear);
    mark(&mut state, 7, 10);
    let before = bytes(&state);
    assert!(state.reduce(stale_marks).is_err());
    for corrupt in 0..3 {
        let TasEditorAction::InputClipboard(
            input_clipboard::TasInputClipboardAction::ApplySpecialTransform(mut action),
        ) = edit_action(&mut state, true, mask, TasSpecialTransform::Clear)
        else {
            unreachable!()
        };
        match corrupt {
            0 => action.target_movie_sha256 = TasDigest([0xE1; 32]),
            1 => action.target_branch_id = "other".to_owned(),
            _ => {
                if let TasSpecialRangeTarget::Marked(snapshot) = &mut action.target {
                    snapshot.ranges = vec![(0, 12)];
                }
            }
        }
        assert!(state.reduce(wrap(action)).is_err());
    }
    assert_eq!(bytes(&state), before);
}

#[test]
fn special_ranges_no_op_and_empty_or_undeclared_masks_do_not_add_history() {
    let (_directory, mut state) = special_input_tests::synthetic_state(
        "gba",
        &[GBA_TILT_DEVICE],
        TasInputFrame::default(),
        BTreeMap::new(),
    );
    mark(&mut state, 0, 3);
    let before = bytes(&state);
    let history = state.session.as_ref().unwrap().undo_count();
    let marks = state
        .input_clipboard
        .marked_ranges
        .snapshot(state.session.as_ref().unwrap())
        .unwrap();
    for transform in [TasSpecialTransform::Clear, TasSpecialTransform::Reverse] {
        let action = edit_action(
            &mut state,
            true,
            TasSpecialInputMask {
                tilt_x: true,
                ..Default::default()
            },
            transform,
        );
        state.reduce(action).unwrap();
        for mask in [
            TasSpecialInputMask::default(),
            TasSpecialInputMask {
                camera: true,
                ..Default::default()
            },
            TasSpecialInputMask {
                zapper: true,
                ..Default::default()
            },
        ] {
            let action = edit_action(&mut state, true, mask, transform);
            assert!(state.reduce(action).is_err());
        }
    }
    assert_eq!(bytes(&state), before);
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history);
    assert_eq!(
        state
            .input_clipboard
            .marked_ranges
            .snapshot(state.session.as_ref().unwrap())
            .unwrap(),
        marks
    );
}

#[test]
fn special_ranges_camera_authoring_failure_in_a_later_interval_is_atomic() {
    let (_directory, mut state) = fixture(
        "gb",
        &[GAME_BOY_MBC7_DEVICE, GAME_BOY_POCKET_CAMERA_DEVICE],
        true,
    );
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| {
            let digest = edit.insert_camera_asset(vec![0xDD; 12]);
            edit.set_input_range(
                "main",
                8,
                1,
                TasInputFrame {
                    camera: TasCameraInput::Blob(digest),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    mark(&mut state, 1, 4);
    mark(&mut state, 7, 10);
    let before = bytes(&state);
    let history = state.session.as_ref().unwrap().undo_count();
    let action = edit_action(
        &mut state,
        true,
        TasSpecialInputMask {
            camera: true,
            ..Default::default()
        },
        TasSpecialTransform::Reverse,
    );
    assert!(state.reduce(action).is_err());
    assert_eq!(bytes(&state), before);
    assert_eq!(state.session.as_ref().unwrap().undo_count(), history);
}

#[test]
fn special_ranges_linked_edit_clears_preview_and_reconstructs_changed_ranges_once() {
    let (_directory, mut state) = special_input_tests::synthetic_state(
        "gba",
        &[GBA_TILT_DEVICE],
        TasInputFrame::default(),
        BTreeMap::new(),
    );
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| {
            edit.insert_frames("main", 3, 9)?;
            edit.set_input_range(
                "main",
                7,
                3,
                TasInputFrame {
                    tilt_x_bits: 0.5_f32.to_bits(),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    mark(&mut state, 1, 4);
    mark(&mut state, 7, 10);
    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 0,
        recording_available: true,
    });
    state
        .install_linked_frame(0, 1, 1, vec![0, 0, 0, 0xFF], (1, 1))
        .unwrap();
    let action = edit_action(
        &mut state,
        true,
        TasSpecialInputMask {
            tilt_x: true,
            ..Default::default()
        },
        TasSpecialTransform::Clear,
    );
    state.reduce(action).unwrap();
    assert!(state.execution_preview.exact_frame().is_none());
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::ReconstructAfterEdit { start: 7, end: 10 }
        ))
    );
    assert_eq!(state.take_pending_host_request(), None);
    let action = edit_action(
        &mut state,
        true,
        TasSpecialInputMask {
            tilt_x: true,
            ..Default::default()
        },
        TasSpecialTransform::Clear,
    );
    state.reduce(action).unwrap();
    assert_eq!(state.take_pending_host_request(), None);
}

#[test]
fn special_ranges_follow_clipboard_recording_discard_and_live_authority_rules() {
    for recording in [false, true] {
        let (_directory, mut state) = fixture("gba", &[GBA_TILT_DEVICE], false);
        select(&mut state, 1, 4);
        let action = edit_action(
            &mut state,
            false,
            TasSpecialInputMask {
                tilt_x: true,
                ..Default::default()
            },
            TasSpecialTransform::Clear,
        );
        let before = bytes(&state);
        if recording {
            state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();
            state.reduce(action).unwrap();
            assert!(state.recording.is_none());
            assert_eq!(
                state
                    .session
                    .as_ref()
                    .unwrap()
                    .selected_branch()
                    .frame_count(),
                12
            );
            state.reduce(TasEditorAction::Undo).unwrap();
        } else {
            state.set_live_status(TasEditorLiveStatus::Staging {
                completed: 0,
                total: 1,
            });
            assert!(
                state
                    .reduce(action)
                    .unwrap_err()
                    .to_string()
                    .contains("live game decision")
            );
        }
        assert_eq!(bytes(&state), before);
    }
}

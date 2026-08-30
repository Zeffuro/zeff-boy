use std::collections::BTreeMap;

use super::*;
use crate::tas_project::{
    TasDeviceIdentity, TasDigest, TasEditorExecutionAttachment,
    TasEditorExecutionUnavailableReason, TasExternalIdentity, TasInitialBranch, TasInputFrame,
    TasProject, TasProjectIdentity,
};
use zeff_emu_common::replay::ReplayStartMetadata;

#[test]
fn editor_presentation_switches_between_embedded_and_separate_hosts() {
    let mut state = TasEditorWindowState::new();
    assert!(!state.open);
    assert_eq!(state.presentation(), TasEditorPresentation::Embedded);
    state.open_separate_window();
    assert!(state.open);
    assert_eq!(state.presentation(), TasEditorPresentation::SeparateWindow);
    state.open_embedded();
    assert!(state.open);
    assert_eq!(state.presentation(), TasEditorPresentation::Embedded);
    state.close();
    assert!(!state.open);
}

#[test]
fn live_staging_accepts_selected_rows_and_the_end_cursor() {
    assert_eq!(live_execution_ui::selected_input_target(0, 1), Some(1));
    assert_eq!(live_execution_ui::selected_input_target(4, 4), Some(4));
    assert_eq!(live_execution_ui::selected_input_target(5, 4), None);
    assert!(live_execution_ui::can_stage_selected_input(0, 1));
    assert!(live_execution_ui::can_stage_selected_input(4, 4));
    assert!(!live_execution_ui::can_stage_selected_input(5, 4));
    assert_eq!(
        live_execution_ui::selected_input_target(
            crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES - 1,
            crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES,
        ),
        Some(crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES)
    );
    assert!(live_execution_ui::can_stage_selected_input(
        crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES - 1,
        crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES,
    ));
    assert_eq!(
        live_execution_ui::selected_input_target(
            crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES,
            crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES + 1,
        ),
        Some(crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES + 1)
    );
    assert!(live_execution_ui::can_stage_selected_input(
        crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES,
        crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES + 1,
    ));
    assert!(!live_execution_ui::can_stage_selected_input(
        u64::MAX,
        u64::MAX
    ));
}

#[test]
fn linked_editor_stays_editable_and_close_keeps_the_linked_game_position() {
    let mut state = TasEditorWindowState::new();
    assert!(
        !TasEditorLiveStatus::Ready {
            recording_available: true
        }
        .locks_editor()
    );
    assert!(
        TasEditorLiveStatus::Staging {
            completed: 0,
            total: 1,
        }
        .locks_editor()
    );
    assert!(TasEditorLiveStatus::AdvancingFrame.locks_editor());
    assert!(TasEditorLiveStatus::Recording.locks_editor());
    assert!(TasEditorLiveStatus::Terminal("worker lost".to_owned()).locks_editor());

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 1,
        recording_available: true,
    });
    assert!(!state.live_status.locks_editor());
    state.close();
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::KeepResultAndReturnToGame
        ))
    );

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 1,
        recording_available: false,
    });
    state.close();
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::KeepResultAndReturnToGame
        ))
    );

    state.set_live_status(TasEditorLiveStatus::AdvancingFrame);
    state.close();
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::ReturnToGameUnchanged
        ))
    );
}

#[test]
fn live_authority_rejects_keyboard_history_and_cursor_mutations() {
    let (_root, mut state) = state_with_project(2);
    state
        .reduce(TasEditorAction::InsertNeutralFrames {
            cursor: 0,
            count: 1,
        })
        .unwrap();
    state.reduce(TasEditorAction::Undo).unwrap();
    assert!(state.session.as_ref().unwrap().can_redo());

    state.set_live_status(TasEditorLiveStatus::Staging {
        completed: 0,
        total: 1,
    });
    assert!(state.reduce(TasEditorAction::Redo).is_err());
    assert!(state.reduce(TasEditorAction::SelectCursor(1)).is_err());
    assert_eq!(state.session.as_ref().unwrap().cursor(), 0);
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .frame_count(),
        2
    );
}

#[test]
fn linked_selection_and_input_edit_request_one_core_reposition() {
    let (_root, mut state) = state_with_project(2);
    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 1,
        recording_available: true,
    });

    state.reduce(TasEditorAction::SelectCursor(1)).unwrap();
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::SeekLinkedInput
        ))
    );
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 1,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::SeekLinkedInput
        ))
    );
}

#[test]
fn separate_window_focus_request_is_one_shot_and_clears_when_embedded_or_closed() {
    let mut state = TasEditorWindowState::new();
    state.open_separate_window();
    assert!(state.take_separate_focus_request());
    assert!(!state.take_separate_focus_request());

    state.open_separate_window();
    state.open_embedded();
    assert!(!state.take_separate_focus_request());

    state.open_separate_window();
    state.close();
    assert!(!state.take_separate_focus_request());
}

#[test]
fn compact_editor_body_remains_scrollable() {
    let (_root, mut state) = state_with_project(8);
    let context = egui::Context::default();
    let mut dimensions = None;
    let _ = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(620.0, 360.0),
            )),
            ..egui::RawInput::default()
        },
        |ui| {
            let mut actions = Vec::new();
            let mut live_action = None;
            let available_height = ui.available_height();
            let output = draw_scrollable_project_content(
                ui,
                &mut state,
                &mut actions,
                &mut live_action,
                available_height,
            );
            dimensions = Some((output.content_size.y, output.inner_rect.height()));
        },
    );

    let (content_height, viewport_height) = dimensions.unwrap();
    assert!(content_height > viewport_height);
}

#[test]
fn timeline_viewport_stays_useful_across_window_sizes() {
    assert_eq!(timeline_height(200.0), MIN_TIMELINE_HEIGHT);
    assert_eq!(timeline_height(600.0), 450.0);
    assert_eq!(timeline_height(1_200.0), MAX_TIMELINE_HEIGHT);
}

fn project(frame_count: u64) -> TasProject {
    let start_state = vec![0xA5; 16];
    TasProject::new(
        "ui-project",
        TasProjectIdentity {
            system: "pce".to_owned(),
            core_family: "test-core".to_owned(),
            determinism_abi: "test-sync-v1".to_owned(),
            source_media_sha256: TasDigest([1; 32]),
            effective_media_sha256: TasDigest([2; 32]),
            patches: Vec::new(),
            firmware: Vec::new(),
            devices: (1..=5)
                .map(|player| TasDeviceIdentity {
                    port: format!("p{player}"),
                    device: "gamepad".to_owned(),
                    configuration_sha256: TasDigest([player as u8; 32]),
                })
                .collect(),
            sync_config_sha256: TasDigest([3; 32]),
            persistent_state: TasExternalIdentity::Absent,
            rtc_state: TasExternalIdentity::Absent,
            sensor_state: TasExternalIdentity::Absent,
            cheats: TasExternalIdentity::Absent,
            state_format_compatibility_id: "test-state-v1".to_owned(),
            start_state_sha256: TasDigest::from_bytes(&start_state),
        },
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )
    .unwrap()
}

pub(super) fn state_with_project(
    frame_count: u64,
) -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let root = crate::test_support::test_directory("tas-editor-ui").unwrap();
    let manual = root.path().join("movie.ztas");
    project(frame_count).save_atomic(&manual).unwrap();
    let mut state = TasEditorWindowState::with_seek_cache_root(root.path().join("seek-cache"));
    state.reduce(TasEditorAction::OpenProject(manual)).unwrap();
    (root, state)
}

#[test]
fn failed_open_project_preserves_session_execution_and_preview() {
    let (root, mut state) = execution_tests::executable_state(2);
    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    let preview = state
        .execution_preview
        .exact_frame()
        .unwrap()
        .rgba()
        .to_vec();

    assert!(
        state
            .open_project(root.path().join("missing.ztas"))
            .is_err()
    );

    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
    assert!(state.execution_engine.is_some());
    assert_eq!(
        state.execution_preview.exact_frame().unwrap().rgba(),
        preview
    );
}

#[test]
fn digital_toggle_is_a_fixed_length_transaction_for_the_selected_player() {
    let (_root, mut state) = state_with_project(4);
    state.attach_execution(TasEditorExecutionAttachment::Unavailable(
        TasEditorExecutionUnavailableReason::NoRunningEmulator,
    ));

    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 2,
            player: 4,
            field: DigitalField::Buttons,
            mask: 1 << 7,
        })
        .unwrap();

    let session = state.session.as_ref().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 4);
    assert_eq!(
        session.selected_branch().input_at(2).players[4].buttons,
        0x80
    );
    assert_eq!(
        session.selected_branch().input_at(1),
        TasInputFrame::default()
    );
    assert_eq!(
        session.selected_branch().input_at(3),
        TasInputFrame::default()
    );
    assert!(session.is_dirty());
}

#[test]
fn failed_media_reevaluation_clears_the_old_engine_but_keeps_editing() {
    let (root, mut state) = state_with_project(1);
    let rom = crate::test_support::build_nes_test_rom();
    let backend = crate::emu_backend::EmuBackend::from_nes(
        zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0).unwrap(),
        root.path().join("old.nes"),
    );
    let identity = state.session.as_ref().unwrap().project().identity().clone();
    state.execution_engine = Some(TasEditorExecutionEngine::new(
        crate::tas_project::verification::TasExecutionSession::new(backend, identity),
    ));

    let replacement = crate::emu_backend::loader::DirectNesTasExecutionLoader::new(
        root.path().join("replacement.nes"),
        Vec::new(),
    );
    state.attach_execution(TasEditorExecutionAttachment::Available(Box::new(
        replacement,
    )));
    assert!(state.execution_engine.is_none());
    assert!(state.session.is_some());
    state
        .reduce(TasEditorAction::InsertNeutralFrames {
            cursor: 0,
            count: 1,
        })
        .unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .frame_count(),
        2
    );
}

#[test]
fn insert_delete_and_fork_actions_preserve_controller_invariants() {
    let (_root, mut state) = state_with_project(3);
    state.reduce(TasEditorAction::SelectCursor(2)).unwrap();
    state
        .reduce(TasEditorAction::InsertNeutralFrames {
            cursor: 2,
            count: 1,
        })
        .unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .frame_count(),
        4
    );

    state
        .reduce(TasEditorAction::ForkBranch {
            id: "route-b".to_owned(),
            name: "Route B".to_owned(),
        })
        .unwrap();
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.selected_branch_id(), "route-b");
    assert_eq!(session.cursor(), 2);
    assert_eq!(session.project().branches().len(), 2);

    state.reduce(TasEditorAction::DeleteFrame(2)).unwrap();
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 3);
    assert_eq!(session.cursor(), 2);
    assert_eq!(session.selected_branch().parent().unwrap().fork_cursor, 2);
}

#[test]
fn frame_recording_grows_the_movie_without_prefilling_neutral_rows() {
    let (_root, mut state) = state_with_project(2);
    state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();

    let session = state.session.as_ref().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 3);
    assert_eq!(session.cursor(), 2);
    assert!(session.selected_branch().input_spans().is_empty());

    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 2,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::CaptureRecordingFrame)
        .unwrap();

    let session = state.session.as_ref().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 4);
    assert_eq!(session.cursor(), 3);
    assert_eq!(session.selected_branch().input_at(2).players[0].buttons, 1);
    assert_eq!(
        session.selected_branch().input_at(3),
        TasInputFrame::default()
    );

    state
        .reduce(TasEditorAction::CaptureRecordingFrame)
        .unwrap();
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 5);
    assert_eq!(session.cursor(), 4);
    assert_eq!(session.selected_branch().input_spans().len(), 1);

    state.reduce(TasEditorAction::StopRecording).unwrap();
    assert!(state.recording.is_none());
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .frame_count(),
        4
    );
}

#[test]
fn bulk_neutral_insertion_is_sparse_and_selects_the_first_new_row() {
    let (_root, mut state) = state_with_project(2);
    state.reduce(TasEditorAction::SelectCursor(2)).unwrap();
    state
        .reduce(TasEditorAction::InsertNeutralFrames {
            cursor: 2,
            count: 600,
        })
        .unwrap();

    let session = state.session.as_ref().unwrap();
    assert_eq!(session.selected_branch().frame_count(), 602);
    assert_eq!(session.cursor(), 2);
    assert!(session.selected_branch().input_spans().is_empty());
}

#[test]
fn leaving_the_active_recording_row_stops_recording() {
    let (_root, mut state) = state_with_project(2);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();
    assert!(state.recording.is_some());

    state.reduce(TasEditorAction::SelectCursor(0)).unwrap();
    assert!(state.recording.is_none());
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .frame_count(),
        2
    );
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
    assert!(!state.session.as_ref().unwrap().is_dirty());
}

#[test]
fn playback_exit_discards_the_unaccepted_recording_row() {
    let (_root, mut state) = state_with_project(2);
    state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();
    assert!(state.reduce(TasEditorAction::ExecuteSeek(3)).is_err());

    let session = state.session.as_ref().unwrap();
    assert!(state.recording.is_none());
    assert_eq!(session.selected_branch().frame_count(), 2);
    assert_eq!(session.cursor(), 0);
}

#[test]
fn save_and_history_actions_require_recording_to_stop_first() {
    let (_root, mut state) = state_with_project(2);
    state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();

    assert!(state.reduce(TasEditorAction::SaveManual).is_err());
    assert!(state.reduce(TasEditorAction::Undo).is_err());
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
}

#[test]
fn advanced_edits_require_recording_to_stop_first() {
    use super::metadata_editor::{TasMetadataAction, TasMetadataMutation};

    let (_root, mut state) = state_with_project(2);
    state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();
    let session = state.session.as_ref().unwrap();
    let before = session.project().encode().unwrap();
    let expected = session.project_content_sha256();

    let result = state.reduce(TasEditorAction::Metadata(TasMetadataAction::new(
        expected,
        TasMetadataMutation::UpsertMarker {
            original_id: None,
            marker: crate::tas_project::TasMarker {
                id: "recording-marker".to_owned(),
                branch_id: "main".to_owned(),
                cursor: 0,
                name: "Recording marker".to_owned(),
            },
        },
    )));

    assert!(result.is_err());
    assert!(state.recording.is_some());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

#[test]
fn player_and_column_models_are_bounded_and_system_specific() {
    let (_root, state) = state_with_project(1);
    let session = state.session.as_ref().unwrap();

    assert_eq!(applicable_player_count(session), 5);
    assert_eq!(digital_columns("nes").len(), 8);
    assert_eq!(digital_columns("gba").len(), 10);
    assert_eq!(digital_columns("pce").len(), 12);
    assert!(
        digital_columns("pce")
            .iter()
            .all(|column| column.mask.count_ones() == 1)
    );
    assert_eq!(player_number("p5"), Some(5));
    assert_eq!(player_number("p6"), None);
    assert_eq!(player_number("controller1"), None);
}

#[test]
fn digital_labels_match_known_backends_and_stay_raw_when_ambiguous() {
    let pce_buttons = digital_columns("pce")
        .into_iter()
        .filter(|column| column.field == DigitalField::Buttons)
        .map(|column| (column.label, column.mask))
        .collect::<Vec<_>>();
    assert_eq!(
        pce_buttons,
        vec![
            ("I", 1),
            ("II", 2),
            ("Sel", 4),
            ("Run", 8),
            ("III", 16),
            ("IV", 32),
            ("V", 64),
            ("VI", 128),
        ]
    );

    for system in ["coleco", "wonderswan", "sms", "unknown"] {
        let columns = digital_columns(system);
        assert_eq!(columns.len(), 16);
        assert_eq!(columns[0].label, "D0");
        assert_eq!(columns[7].label, "D7");
        assert_eq!(columns[8].label, "B0");
        assert_eq!(columns[15].label, "B7");
    }
}

#[test]
fn failed_actions_leave_the_session_unchanged() {
    let (_root, mut state) = state_with_project(1);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();

    assert!(
        state
            .reduce(TasEditorAction::ToggleDigital {
                cursor: 1,
                player: 0,
                field: DigitalField::Buttons,
                mask: 1,
            })
            .is_err()
    );
    assert!(
        state
            .reduce(TasEditorAction::ForkBranch {
                id: "bad id".to_owned(),
                name: "Bad".to_owned(),
            })
            .is_err()
    );

    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.selected_branch_id(), "main");
    assert_eq!(session.cursor(), 0);
}

#[test]
fn undo_and_redo_actions_restore_exact_ui_session_snapshots() {
    let (_root, mut state) = state_with_project(2);
    state.reduce(TasEditorAction::SelectCursor(1)).unwrap();
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 1,
            player: 0,
            field: DigitalField::Dpad,
            mask: 1 << 2,
        })
        .unwrap();
    assert!(state.session.as_ref().unwrap().can_undo());

    state.reduce(TasEditorAction::Undo).unwrap();
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.cursor(), 1);
    assert_eq!(
        session.selected_branch().input_at(1),
        TasInputFrame::default()
    );
    assert!(session.can_redo());

    state.reduce(TasEditorAction::Redo).unwrap();
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.cursor(), 1);
    assert_eq!(
        session.selected_branch().input_at(1).players[0].dpad,
        1 << 2
    );
}

#[test]
fn persistence_actions_keep_manual_and_autosave_witnesses_separate() {
    let (_root, mut state) = state_with_project(2);
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 0,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    state.reduce(TasEditorAction::Autosave).unwrap();
    let autosaved_generation = state.session.as_ref().unwrap().last_autosaved_generation();

    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 0,
            player: 0,
            field: DigitalField::Dpad,
            mask: 1 << 1,
        })
        .unwrap();
    assert!(state.session.as_ref().unwrap().is_dirty());
    state.reduce(TasEditorAction::RecoverAutosave).unwrap();

    let recovered = state.session.as_ref().unwrap();
    assert_eq!(recovered.source(), TasEditorSessionSource::Autosave);
    assert_eq!(
        recovered.project().edit_generation(),
        autosaved_generation.unwrap()
    );
    assert_eq!(
        recovered.selected_branch().input_at(0).players[0].buttons,
        1
    );
    assert_eq!(recovered.selected_branch().input_at(0).players[0].dpad, 0);
    assert!(recovered.is_dirty());

    state.reduce(TasEditorAction::SaveManual).unwrap();
    assert!(!state.session.as_ref().unwrap().is_dirty());
}

#[test]
fn periodic_autosave_runs_while_closed_without_duplicate_generations() {
    use std::time::Duration;

    let (_root, mut state) = state_with_project(2);
    state.open = false;
    state.test_tick_periodic_autosave_at(Duration::ZERO);
    state.test_tick_periodic_autosave_at(Duration::from_secs(30));

    let session = state.session.as_ref().unwrap();
    assert_eq!(
        session.last_autosaved_generation(),
        Some(session.project().edit_generation())
    );
    let first_message = state.message.clone();

    state.test_tick_periodic_autosave_at(Duration::from_secs(60));
    assert_eq!(state.message, first_message);
}

use std::collections::BTreeMap;

use zeff_emu_common::replay::{ReplayEvent, ReplayStartMetadata};

use super::event_editor::{TasEventAction, TasEventMutation, can_author_fds_events};
use super::*;
use crate::tas_project::{
    TasDeviceIdentity, TasDigest, TasExternalIdentity, TasFirmwareIdentity, TasInitialBranch,
    TasProject, TasProjectIdentity,
};

fn fds_project(frame_count: u64, events: Vec<ReplayEvent>) -> TasProject {
    project_with_firmware(
        frame_count,
        events,
        vec![TasFirmwareIdentity::External {
            firmware_id: "nintendo.fds.bios".to_owned(),
            variant: Some("test".to_owned()),
            sha256: TasDigest([2; 32]),
        }],
    )
}

fn project_with_firmware(
    frame_count: u64,
    events: Vec<ReplayEvent>,
    firmware: Vec<TasFirmwareIdentity>,
) -> TasProject {
    let start_state = vec![0xD5; 16];
    TasProject::new(
        "ui-fds-events",
        TasProjectIdentity {
            system: "nes".to_owned(),
            core_family: "nes-test".to_owned(),
            determinism_abi: "nes-test-sync-v1".to_owned(),
            source_media_sha256: TasDigest([1; 32]),
            effective_media_sha256: TasDigest([1; 32]),
            patches: Vec::new(),
            firmware,
            devices: vec![TasDeviceIdentity {
                port: "p1".to_owned(),
                device: "nes-standard-controller".to_owned(),
                configuration_sha256: TasDigest([4; 32]),
            }],
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
    .unwrap()
}

fn fds_state(
    frame_count: u64,
    events: Vec<ReplayEvent>,
) -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    state_for_project(fds_project(frame_count, events))
}

fn state_for_project(
    project: TasProject,
) -> (crate::test_support::TestDirectory, TasEditorWindowState) {
    let root = crate::test_support::test_directory("tas-editor-fds-events").unwrap();
    let manual = root.path().join("movie.ztas");
    project.save_atomic(&manual).unwrap();
    let mut state = TasEditorWindowState::with_seek_cache_root(root.path().join("seek-cache"));
    state.reduce(TasEditorAction::OpenProject(manual)).unwrap();
    (root, state)
}

fn action(state: &TasEditorWindowState, mutation: TasEventMutation) -> TasEditorAction {
    TasEditorAction::Event(TasEventAction::new(
        state.session.as_ref().unwrap().project_content_sha256(),
        mutation,
    ))
}

#[test]
fn fds_add_update_remove_are_canonical_and_accept_the_terminal_boundary() {
    let (_root, mut state) = fds_state(3, vec![ReplayEvent::FdsDiskSide { frame: 2, side: 2 }]);
    let initial_rerecords = state.session.as_ref().unwrap().project().rerecord_count();

    let add_terminal = action(
        &state,
        TasEventMutation::Add {
            branch_id: "main".to_owned(),
            frame: 3,
            side: 3,
        },
    );
    state.reduce(add_terminal).unwrap();
    let add_earlier = action(
        &state,
        TasEventMutation::Add {
            branch_id: "main".to_owned(),
            frame: 0,
            side: 0,
        },
    );
    state.reduce(add_earlier).unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .events()
            .iter()
            .map(ReplayEvent::frame)
            .collect::<Vec<_>>(),
        vec![0, 2, 3]
    );

    let expected = state.session.as_ref().unwrap().selected_branch().events()[1].clone();
    let update = action(
        &state,
        TasEventMutation::Update {
            branch_id: "main".to_owned(),
            canonical_index: 1,
            expected_event: expected,
            frame: 1,
            side: u8::MAX,
        },
    );
    state.reduce(update).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().selected_branch().events(),
        &[
            ReplayEvent::FdsDiskSide { frame: 0, side: 0 },
            ReplayEvent::FdsDiskSide {
                frame: 1,
                side: u8::MAX,
            },
            ReplayEvent::FdsDiskSide { frame: 3, side: 3 },
        ]
    );

    let terminal = state.session.as_ref().unwrap().selected_branch().events()[2].clone();
    let remove = action(
        &state,
        TasEventMutation::Remove {
            branch_id: "main".to_owned(),
            canonical_index: 2,
            expected_event: terminal,
        },
    );
    state.reduce(remove).unwrap();
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().rerecord_count(), initial_rerecords + 4);
    assert!(session.can_undo());
    assert!(session.is_dirty());
    let generation = session.project().edit_generation();
    state.reduce(TasEditorAction::Autosave).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().last_autosaved_generation(),
        Some(generation)
    );
}

#[test]
fn stale_branch_index_and_range_failures_roll_back_exactly() {
    let (_root, mut state) = fds_state(2, vec![ReplayEvent::FdsDiskSide { frame: 1, side: 1 }]);
    let stale = action(
        &state,
        TasEventMutation::Update {
            branch_id: "main".to_owned(),
            canonical_index: 0,
            expected_event: ReplayEvent::FdsDiskSide { frame: 1, side: 1 },
            frame: 0,
            side: 2,
        },
    );
    state
        .reduce(TasEditorAction::InsertNeutralFrames {
            cursor: 0,
            count: 1,
        })
        .unwrap();
    let before_stale = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(stale).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before_stale
    );

    let before_branch = before_stale.clone();
    let wrong_branch = action(
        &state,
        TasEventMutation::Add {
            branch_id: "other".to_owned(),
            frame: 0,
            side: 0,
        },
    );
    assert!(state.reduce(wrong_branch).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before_branch
    );

    let out_of_range = action(
        &state,
        TasEventMutation::Add {
            branch_id: "main".to_owned(),
            frame: 4,
            side: 0,
        },
    );
    assert!(state.reduce(out_of_range).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before_branch
    );

    let wrong_expected = action(
        &state,
        TasEventMutation::Remove {
            branch_id: "main".to_owned(),
            canonical_index: 0,
            expected_event: ReplayEvent::FdsDiskSide { frame: 0, side: 9 },
        },
    );
    assert!(state.reduce(wrong_expected).is_err());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before_branch
    );
}

#[test]
fn add_requires_exact_external_fds_firmware_but_existing_events_remain_repairable() {
    let project = fds_project(1, Vec::new());
    assert!(can_author_fds_events(project.identity()));

    let mut wrong_system = project.identity().clone();
    wrong_system.system = "gb".to_owned();
    assert!(!can_author_fds_events(&wrong_system));

    let mut skipped = project.identity().clone();
    skipped.firmware = vec![TasFirmwareIdentity::Skipped {
        firmware_id: "nintendo.fds.bios".to_owned(),
        compatibility_version: 1,
    }];
    assert!(!can_author_fds_events(&skipped));

    let unsupported_existing = project_with_firmware(
        1,
        vec![ReplayEvent::FdsDiskSide { frame: 1, side: 7 }],
        Vec::new(),
    );
    assert!(!can_author_fds_events(unsupported_existing.identity()));
    let (_root, mut state) = state_for_project(unsupported_existing);
    let expected = state.session.as_ref().unwrap().selected_branch().events()[0].clone();
    let repair = action(
        &state,
        TasEventMutation::Update {
            branch_id: "main".to_owned(),
            canonical_index: 0,
            expected_event: expected,
            frame: 1,
            side: 6,
        },
    );
    state.reduce(repair).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().selected_branch().events(),
        &[ReplayEvent::FdsDiskSide { frame: 1, side: 6 }]
    );
}

#[test]
fn failed_and_stale_actions_preserve_the_exact_private_preview() {
    let (_root, mut state) = super::execution_tests::executable_state(2);
    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    let before_preview = state.execution_preview.exact_frame().unwrap().clone();
    let before_project = state.session.as_ref().unwrap().project().encode().unwrap();

    let unsupported_add = action(
        &state,
        TasEventMutation::Add {
            branch_id: "main".to_owned(),
            frame: 1,
            side: 0,
        },
    );
    assert!(state.reduce(unsupported_add).is_err());
    assert_eq!(
        state.execution_preview.exact_frame().unwrap(),
        &before_preview
    );
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before_project
    );

    let stale = TasEventAction::new(
        state.session.as_ref().unwrap().project_content_sha256(),
        TasEventMutation::Add {
            branch_id: "main".to_owned(),
            frame: 1,
            side: 0,
        },
    );
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 0,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    let current_preview = state.execution_preview.exact_frame().unwrap().clone();
    let current_project = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(state.reduce(TasEditorAction::Event(stale)).is_err());
    assert_eq!(
        state.execution_preview.exact_frame().unwrap(),
        &current_preview
    );
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        current_project
    );
}

#[test]
fn successful_event_edit_immediately_detaches_an_incompatible_private_engine() {
    let (_root, mut state) = super::execution_tests::executable_state(2);
    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    assert!(state.execution_preview.exact_frame().is_some());
    state
        .session
        .as_mut()
        .unwrap()
        .edit_transaction(|edit| {
            edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 1, side: 0 }])
        })
        .unwrap();
    assert!(state.execution_engine.is_some());

    let expected = state.session.as_ref().unwrap().selected_branch().events()[0].clone();
    let update = action(
        &state,
        TasEventMutation::Update {
            branch_id: "main".to_owned(),
            canonical_index: 0,
            expected_event: expected,
            frame: 2,
            side: 1,
        },
    );
    let message = state.reduce(update).unwrap().unwrap();

    assert!(state.execution_engine.is_none());
    assert!(state.execution_preview.exact_frame().is_none());
    assert!(message.contains("private execution detached"));
    assert_eq!(
        state.session.as_ref().unwrap().selected_branch().events(),
        &[ReplayEvent::FdsDiskSide { frame: 2, side: 1 }]
    );
}

use super::execution_tests::executable_state;
use super::metadata_editor::{TasMetadataAction, TasMetadataMutation};
use super::*;
use crate::tas_project::{TasAnnotation, TasMarker};

fn reduce_metadata(
    state: &mut TasEditorWindowState,
    mutation: TasMetadataMutation,
) -> anyhow::Result<Option<String>> {
    let expected = state
        .session
        .as_ref()
        .expect("metadata test requires an open session")
        .project_content_sha256();
    state.reduce(TasEditorAction::Metadata(TasMetadataAction::new(
        expected, mutation,
    )))
}

#[test]
fn marker_add_update_remove_are_undoable_presentation_transactions() {
    let (_root, mut state) = executable_state(4);
    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    assert!(state.execution_preview.exact_frame().is_some());

    reduce_metadata(
        &mut state,
        TasMetadataMutation::UpsertMarker {
            original_id: None,
            marker: TasMarker {
                id: "setup".to_owned(),
                branch_id: "main".to_owned(),
                cursor: 1,
                name: "Initial setup".to_owned(),
            },
        },
    )
    .unwrap();
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().edit_generation(), 1);
    assert_eq!(session.project().rerecord_count(), 0);
    assert_eq!(session.project().markers()[0].id, "setup");
    assert!(session.is_dirty());
    assert!(session.can_undo());
    assert!(state.execution_preview.exact_frame().is_none());

    state.reduce(TasEditorAction::Undo).unwrap();
    assert!(
        state
            .session
            .as_ref()
            .unwrap()
            .project()
            .markers()
            .is_empty()
    );
    state.reduce(TasEditorAction::Redo).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().project().markers()[0].id,
        "setup"
    );

    reduce_metadata(
        &mut state,
        TasMetadataMutation::UpsertMarker {
            original_id: Some("setup".to_owned()),
            marker: TasMarker {
                id: "boss".to_owned(),
                branch_id: "main".to_owned(),
                cursor: 3,
                name: "Boss entry".to_owned(),
            },
        },
    )
    .unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().project().markers(),
        &[TasMarker {
            id: "boss".to_owned(),
            branch_id: "main".to_owned(),
            cursor: 3,
            name: "Boss entry".to_owned(),
        }]
    );

    reduce_metadata(
        &mut state,
        TasMetadataMutation::RemoveMarker {
            branch_id: "main".to_owned(),
            id: "boss".to_owned(),
        },
    )
    .unwrap();
    assert!(
        state
            .session
            .as_ref()
            .unwrap()
            .project()
            .markers()
            .is_empty()
    );
    state.reduce(TasEditorAction::Undo).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().project().markers()[0].id,
        "boss"
    );
}

#[test]
fn annotation_add_update_remove_preserve_autosave_and_history_witnesses() {
    let (_root, mut state) = executable_state(4);
    reduce_metadata(
        &mut state,
        TasMetadataMutation::UpsertAnnotation {
            original_id: None,
            annotation: TasAnnotation {
                id: "route".to_owned(),
                branch_id: "main".to_owned(),
                start: 1,
                length: 2,
                kind: "note".to_owned(),
                text: "First route".to_owned(),
            },
        },
    )
    .unwrap();
    state.reduce(TasEditorAction::Autosave).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().last_autosaved_generation(),
        Some(1)
    );

    reduce_metadata(
        &mut state,
        TasMetadataMutation::UpsertAnnotation {
            original_id: Some("route".to_owned()),
            annotation: TasAnnotation {
                id: "lag-window".to_owned(),
                branch_id: "main".to_owned(),
                start: 0,
                length: 1,
                kind: "lag".to_owned(),
                text: "One frame".to_owned(),
            },
        },
    )
    .unwrap();
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().edit_generation(), 2);
    assert_eq!(session.project().rerecord_count(), 0);
    assert_eq!(session.project().annotations()[0].id, "lag-window");
    assert_eq!(session.last_autosaved_generation(), Some(1));

    state.reduce(TasEditorAction::Autosave).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().last_autosaved_generation(),
        Some(2)
    );
    reduce_metadata(
        &mut state,
        TasMetadataMutation::RemoveAnnotation {
            branch_id: "main".to_owned(),
            id: "lag-window".to_owned(),
        },
    )
    .unwrap();
    assert!(
        state
            .session
            .as_ref()
            .unwrap()
            .project()
            .annotations()
            .is_empty()
    );
    state.reduce(TasEditorAction::Undo).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().project().annotations()[0].id,
        "lag-window"
    );
    state.reduce(TasEditorAction::Redo).unwrap();
    assert!(
        state
            .session
            .as_ref()
            .unwrap()
            .project()
            .annotations()
            .is_empty()
    );
}

#[test]
fn invalid_annotation_rolls_back_without_clearing_the_exact_preview() {
    let (_root, mut state) = executable_state(4);
    state.reduce(TasEditorAction::ExecuteSeek(1)).unwrap();
    let before = state.session.as_ref().unwrap().project().encode().unwrap();

    let error = reduce_metadata(
        &mut state,
        TasMetadataMutation::UpsertAnnotation {
            original_id: None,
            annotation: TasAnnotation {
                id: "past-end".to_owned(),
                branch_id: "main".to_owned(),
                start: 4,
                length: 1,
                kind: "note".to_owned(),
                text: "invalid".to_owned(),
            },
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("past its branch end"));
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.project().edit_generation(), 0);
    assert!(!session.can_undo());
    assert!(state.execution_preview.exact_frame().is_some());
}

#[test]
fn metadata_action_is_rejected_after_the_project_revision_changes() {
    let (_root, mut state) = executable_state(4);
    let expected = state.session.as_ref().unwrap().project_content_sha256();
    let stale = TasMetadataAction::new(
        expected,
        TasMetadataMutation::UpsertMarker {
            original_id: None,
            marker: TasMarker {
                id: "stale".to_owned(),
                branch_id: "main".to_owned(),
                cursor: 1,
                name: "Must not land".to_owned(),
            },
        },
    );
    state
        .reduce(TasEditorAction::InsertNeutralFrames {
            cursor: 0,
            count: 1,
        })
        .unwrap();
    let after_timeline_edit = state.session.as_ref().unwrap().project().encode().unwrap();

    let error = state.reduce(TasEditorAction::Metadata(stale)).unwrap_err();

    assert!(error.to_string().contains("project changed"));
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        after_timeline_edit
    );
    assert!(
        state
            .session
            .as_ref()
            .unwrap()
            .project()
            .markers()
            .is_empty()
    );
}

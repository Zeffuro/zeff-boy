use super::*;

fn add_deletable_subtree(state: &mut TasEditorWindowState) {
    state
        .reduce(TasEditorAction::ForkBranch {
            id: "route".to_owned(),
            name: "Route".to_owned(),
        })
        .unwrap();
    state
        .reduce(TasEditorAction::ForkBranch {
            id: "child".to_owned(),
            name: "Child".to_owned(),
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SelectBranch("main".to_owned()))
        .unwrap();
}

#[test]
fn delete_action_removes_a_subtree_and_undo_restores_it() {
    let (_root, mut state) = state_with_project(4);
    add_deletable_subtree(&mut state);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();

    let message = state
        .reduce(TasEditorAction::DeleteBranchSubtree {
            id: "route".to_owned(),
        })
        .unwrap();

    assert_eq!(
        message.as_deref(),
        Some("Deleted branch Route and 1 descendants")
    );
    let session = state.session.as_ref().unwrap();
    assert!(session.project().branch("route").is_none());
    assert!(session.project().branch("child").is_none());
    assert_eq!(session.selected_branch_id(), "main");

    state.reduce(TasEditorAction::Undo).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

#[test]
fn delete_action_obeys_recording_and_live_authority_gates() {
    let (_root, mut state) = state_with_project(4);
    add_deletable_subtree(&mut state);
    let before = state.session.as_ref().unwrap().project().encode().unwrap();

    state.reduce(TasEditorAction::StartRecordingAtEnd).unwrap();
    let recording_error = state
        .reduce(TasEditorAction::DeleteBranchSubtree {
            id: "route".to_owned(),
        })
        .unwrap_err();
    assert!(recording_error.to_string().contains("stop frame recording"));
    state.reduce(TasEditorAction::StopRecording).unwrap();
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 0,
        recording_available: true,
    });
    let live_error = state
        .reduce(TasEditorAction::DeleteBranchSubtree {
            id: "route".to_owned(),
        })
        .unwrap_err();
    assert!(live_error.to_string().contains("live game decision"));
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );
}

#[test]
fn delete_action_rejects_the_active_route_without_history_mutation() {
    let (_root, mut state) = state_with_project(4);
    state
        .reduce(TasEditorAction::ForkBranch {
            id: "route".to_owned(),
            name: "Route".to_owned(),
        })
        .unwrap();
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    let undo_count = state.session.as_ref().unwrap().undo_count();

    let error = state
        .reduce(TasEditorAction::DeleteBranchSubtree {
            id: "route".to_owned(),
        })
        .unwrap_err();

    assert!(error.to_string().contains("active TAS branch"));
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.undo_count(), undo_count);
}

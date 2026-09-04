use super::super::*;
use super::{attach_current_verification, project};

fn project_with_branch_tree() -> TasProject {
    let mut project = project();
    project
        .edit_transaction(|edit| {
            edit.fork_branch("alternate", 5, "alternate-child", "Alternate Child")?;
            edit.fork_branch("main", 7, "safe-route", "Safe Route")?;
            Ok(())
        })
        .unwrap();
    project.markers.extend([
        TasMarker {
            id: "alternate-marker".to_owned(),
            branch_id: "alternate".to_owned(),
            cursor: 3,
            name: "Alternate".to_owned(),
        },
        TasMarker {
            id: "child-marker".to_owned(),
            branch_id: "alternate-child".to_owned(),
            cursor: 4,
            name: "Child".to_owned(),
        },
    ]);
    project.annotations.push(TasAnnotation {
        id: "child-note".to_owned(),
        branch_id: "alternate-child".to_owned(),
        start: 1,
        length: 2,
        kind: "note".to_owned(),
        text: "child note".to_owned(),
    });
    project.validate().unwrap();
    project
}

#[test]
fn deletion_removes_the_complete_subtree_and_its_presentation_records() {
    let mut project = project_with_branch_tree();
    attach_current_verification(&mut project, "main");
    attach_current_verification(&mut project, "safe-route");
    let main = project.branch("main").unwrap().clone();
    let safe_route = project.branch("safe-route").unwrap().clone();
    let generation = project.edit_generation();
    let rerecords = project.rerecord_count();

    let mut deleted = 0;
    let outcome = project
        .edit_transaction(|edit| {
            deleted = edit.delete_branch_subtree("alternate")?;
            Ok(())
        })
        .unwrap();

    assert_eq!(deleted, 2);
    assert!(outcome.changed);
    assert!(outcome.branch_impacts.is_empty());
    assert_eq!(project.edit_generation(), generation + 1);
    assert_eq!(project.rerecord_count(), rerecords);
    assert_eq!(project.branch("main"), Some(&main));
    assert_eq!(project.branch("safe-route"), Some(&safe_route));
    assert!(project.branch("alternate").is_none());
    assert!(project.branch("alternate-child").is_none());
    assert!(
        project
            .markers()
            .iter()
            .all(|marker| marker.branch_id == "main")
    );
    assert!(project.annotations().iter().all(|annotation| {
        annotation.branch_id != "alternate" && annotation.branch_id != "alternate-child"
    }));
    project.validate().unwrap();
    assert_eq!(
        TasProject::decode(&project.encode().unwrap()).unwrap(),
        project
    );
}

#[test]
fn root_active_and_active_ancestor_deletions_are_rejected_atomically() {
    let mut project = project_with_branch_tree();
    let root_bytes = project.encode().unwrap();
    let root_error = project
        .edit_transaction(|edit| edit.delete_branch_subtree("main").map(|_| ()))
        .unwrap_err();
    assert!(root_error.to_string().contains("root TAS branch"));
    assert_eq!(project.encode().unwrap(), root_bytes);

    project
        .edit_transaction(|edit| edit.set_active_branch("alternate-child"))
        .unwrap();
    let active_bytes = project.encode().unwrap();
    let active_error = project
        .edit_transaction(|edit| edit.delete_branch_subtree("alternate-child").map(|_| ()))
        .unwrap_err();
    assert!(active_error.to_string().contains("active TAS branch"));
    assert_eq!(project.encode().unwrap(), active_bytes);

    let ancestor_error = project
        .edit_transaction(|edit| edit.delete_branch_subtree("alternate").map(|_| ()))
        .unwrap_err();
    assert!(ancestor_error.to_string().contains("ancestors"));
    assert_eq!(project.encode().unwrap(), active_bytes);
}

#[test]
fn editor_history_restores_and_reapplies_exact_subtree_deletion() {
    let root = crate::test_support::test_directory("tas-branch-deletion-history").unwrap();
    let manual = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let mut session =
        TasEditorSession::new(project_with_branch_tree(), &manual, autosaves, seek_cache).unwrap();
    session.set_cursor(6).unwrap();
    let before = session.project().encode().unwrap();

    session
        .edit_transaction(|edit| edit.delete_branch_subtree("alternate").map(|_| ()))
        .unwrap();
    let deleted = session.project().encode().unwrap();
    assert_ne!(deleted, before);
    assert_eq!(session.selected_branch_id(), "main");
    assert_eq!(session.cursor(), 6);

    assert!(session.undo().unwrap());
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.selected_branch_id(), "main");
    assert_eq!(session.cursor(), 6);

    assert!(session.redo().unwrap());
    assert_eq!(session.project().encode().unwrap(), deleted);
    assert_eq!(session.selected_branch_id(), "main");
    assert_eq!(session.cursor(), 6);
}

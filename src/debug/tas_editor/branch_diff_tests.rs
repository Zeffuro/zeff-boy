use super::*;
fn add_branch(state: &mut TasEditorWindowState, id: &str, name: &str) {
    state
        .reduce(TasEditorAction::ForkBranch {
            id: id.to_owned(),
            name: name.to_owned(),
        })
        .unwrap();
}

#[test]
fn branch_diff_target_is_transient_and_defaults_to_the_parent() {
    let (_root, mut state) = tests::state_with_project(4);
    state.reduce(TasEditorAction::SelectCursor(2)).unwrap();
    add_branch(&mut state, "child", "Child");
    let session = state.session.as_ref().unwrap();
    let project_before = session.project().encode().unwrap();
    let dirty_before = session.is_dirty();
    let cursor_before = session.cursor();
    assert_eq!(
        state.branch_diff_editor.selected_target(session),
        Some("main")
    );
    assert_eq!(session.project().encode().unwrap(), project_before);
    assert_eq!(session.is_dirty(), dirty_before);
    assert_eq!(session.cursor(), cursor_before);
    assert!(state.execution_preview.exact_frame().is_none());
}

#[test]
fn branch_diff_cache_tracks_exact_project_sha_and_bounded_core_rows() {
    let (_root, mut state) = tests::state_with_project(4);
    add_branch(&mut state, "target", "Target");
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 1,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SelectBranch("main".to_owned()))
        .unwrap();
    {
        let session = state.session.as_ref().unwrap();
        let diff = state.branch_diff_editor.diff(session).unwrap();
        assert!(diff.input_hunks.len() <= crate::tas_project::MAX_BRANCH_DIFF_RETAINED_HUNKS);
    }
    let before = state.branch_diff_editor.cached_diff().unwrap().clone();
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 2,
            player: 0,
            field: DigitalField::Dpad,
            mask: 1,
        })
        .unwrap();
    let session = state.session.as_ref().unwrap();
    let after = state.branch_diff_editor.diff(session).unwrap();
    assert_ne!(before.source_movie_sha256, after.source_movie_sha256);
}

#[test]
fn undo_recomputes_diff_for_a_restored_sha() {
    let (_root, mut state) = tests::state_with_project(4);
    add_branch(&mut state, "target", "Target");
    state
        .reduce(TasEditorAction::SelectBranch("main".to_owned()))
        .unwrap();
    let original_sha = state.session.as_ref().unwrap().project_content_sha256();
    {
        let session = state.session.as_ref().unwrap();
        state.branch_diff_editor.diff(session).unwrap();
    }
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 0,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    state.reduce(TasEditorAction::Undo).unwrap();
    let session = state.session.as_ref().unwrap();
    assert_eq!(session.project_content_sha256(), original_sha);
    state.branch_diff_editor.diff(session).unwrap();
    assert!(state.branch_diff_editor.cached_diff().is_some());
}

#[test]
fn autosave_recovery_discards_a_pre_recovery_diff_cache() {
    let (_root, mut state) = tests::state_with_project(4);
    add_branch(&mut state, "target", "Target");
    state
        .reduce(TasEditorAction::SelectBranch("main".to_owned()))
        .unwrap();
    state.reduce(TasEditorAction::Autosave).unwrap();
    let source_before_edit = {
        let session = state.session.as_ref().unwrap();
        state
            .branch_diff_editor
            .diff(session)
            .unwrap()
            .source_movie_sha256
    };
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 0,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    let source_after_edit = {
        let session = state.session.as_ref().unwrap();
        state
            .branch_diff_editor
            .diff(session)
            .unwrap()
            .source_movie_sha256
    };
    assert_ne!(source_before_edit, source_after_edit);
    state.reduce(TasEditorAction::RecoverAutosave).unwrap();
    let session = state.session.as_ref().unwrap();
    let recovered = state.branch_diff_editor.diff(session).unwrap();
    assert_eq!(recovered.source_movie_sha256, source_before_edit);
}

#[test]
fn jump_rechecks_its_witness_and_is_the_only_comparison_side_effect() {
    let (_root, mut state) = tests::state_with_project(4);
    state.reduce(TasEditorAction::SelectCursor(1)).unwrap();
    add_branch(&mut state, "target", "Target");
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 1,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SelectBranch("main".to_owned()))
        .unwrap();
    let action = {
        let session = state.session.as_ref().unwrap();
        let diff = state.branch_diff_editor.diff(session).unwrap();
        let hunk = diff.input_hunks.first().unwrap();
        branch_diff_editor::TasBranchDiffJumpAction::new(
            session.project_content_sha256(),
            session.selected_branch_id().to_owned(),
            diff.source_movie_sha256,
            hunk.start,
        )
    };
    let before = state.session.as_ref().unwrap().project().encode().unwrap();
    state
        .reduce(TasEditorAction::JumpToBranchDiffHunk(action.clone()))
        .unwrap();
    assert_eq!(state.session.as_ref().unwrap().selected_branch_id(), "main");
    assert_eq!(state.session.as_ref().unwrap().cursor(), action.cursor());
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before
    );

    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 0,
            player: 0,
            field: DigitalField::Dpad,
            mask: 1,
        })
        .unwrap();
    let before_stale = state.session.as_ref().unwrap().project().encode().unwrap();
    assert!(
        state
            .reduce(TasEditorAction::JumpToBranchDiffHunk(action))
            .is_err()
    );
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before_stale
    );
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .selected_branch()
            .input_at(0)
            .players[0]
            .dpad,
        1
    );
}

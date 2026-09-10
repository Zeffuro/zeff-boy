use super::*;
use crate::tas_project::TasBranchDiff;

const UI_SIZE: egui::Vec2 = egui::vec2(640.0, 480.0);

fn test_context() -> egui::Context {
    let context = egui::Context::default();
    context.global_style_mut(|style| style.animation_time = 0.0);
    context
}

fn raw_input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, UI_SIZE)),
        events,
        ..egui::RawInput::default()
    }
}

fn render_branch_diff(
    context: &egui::Context,
    state: &mut TasEditorWindowState,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Vec<TasEditorAction>) {
    let mut actions = Vec::new();
    let output = context.run_ui(raw_input(events), |ui| {
        let session = state.session.as_ref().unwrap();
        branch_diff_editor::draw_branch_diff_editor(
            ui,
            session,
            &mut state.branch_diff_editor,
            &mut actions,
        );
    });
    (output, actions)
}

fn label_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text.starts_with(label)
            {
                Some(text.pos + egui::vec2(8.0, text.galley.size().y * 0.5))
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("could not find {label:?} in branch diff"))
}

fn pointer_event(position: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn wait_for_diff(
    state: &mut branch_diff_editor::TasBranchDiffEditorState,
    session: &TasEditorSession,
) -> TasBranchDiff {
    let context = egui::Context::default();
    for _ in 0..1_000 {
        match state.refresh(session, &context, true) {
            branch_diff_editor::TasBranchDiffPresentation::Ready(diff) => return diff.clone(),
            branch_diff_editor::TasBranchDiffPresentation::Failed(error) => {
                panic!("branch diff failed: {error}")
            }
            branch_diff_editor::TasBranchDiffPresentation::NoTarget => {
                panic!("branch diff unexpectedly has no target")
            }
            branch_diff_editor::TasBranchDiffPresentation::Pending => {
                std::thread::sleep(std::time::Duration::from_millis(1))
            }
        }
    }
    panic!("background branch diff did not finish")
}

fn add_branch(state: &mut TasEditorWindowState, id: &str, name: &str) {
    state
        .reduce(TasEditorAction::ForkBranch {
            id: id.to_owned(),
            name: name.to_owned(),
        })
        .unwrap();
}

#[test]
fn expanded_pending_branch_diff_renders_its_status_without_actions() {
    let (_root, mut state) = tests::state_with_project(4);
    add_branch(&mut state, "target", "Target");
    let context = test_context();
    let (output, actions) = render_branch_diff(&context, &mut state, Vec::new());
    assert!(actions.is_empty());
    let header = label_position(&output, "Branch diff");
    let (_, actions) = render_branch_diff(&context, &mut state, pointer_event(header, true));
    assert!(actions.is_empty());
    let (output, actions) = render_branch_diff(&context, &mut state, pointer_event(header, false));
    assert!(actions.is_empty());
    assert!(output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Comparing branches…")
    }));
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
        let diff = wait_for_diff(&mut state.branch_diff_editor, session);
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
    let after = wait_for_diff(&mut state.branch_diff_editor, session);
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
        wait_for_diff(&mut state.branch_diff_editor, session);
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
    wait_for_diff(&mut state.branch_diff_editor, session);
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
        wait_for_diff(&mut state.branch_diff_editor, session).source_movie_sha256
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
        wait_for_diff(&mut state.branch_diff_editor, session).source_movie_sha256
    };
    assert_ne!(source_before_edit, source_after_edit);
    state.reduce(TasEditorAction::RecoverAutosave).unwrap();
    let session = state.session.as_ref().unwrap();
    let recovered = wait_for_diff(&mut state.branch_diff_editor, session);
    assert_eq!(recovered.source_movie_sha256, source_before_edit);
}

#[test]
fn jump_selects_and_reveals_a_distant_source_hunk_without_editing_the_project() {
    const HUNK_FRAME: u64 = 200_000;

    let (_root, mut state) = tests::state_with_project(HUNK_FRAME + 1);
    add_branch(&mut state, "target", "Target");
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: HUNK_FRAME,
            player: 0,
            field: DigitalField::Buttons,
            mask: 1,
        })
        .unwrap();
    state
        .reduce(TasEditorAction::SelectBranch("main".to_owned()))
        .unwrap();
    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 0,
            extend_selection: false,
        })
        .unwrap();
    let action = {
        let session = state.session.as_ref().unwrap();
        let diff = wait_for_diff(&mut state.branch_diff_editor, session);
        let hunk = diff.input_hunks.first().unwrap();
        assert_eq!(hunk.start, HUNK_FRAME);
        branch_diff_editor::TasBranchDiffJumpAction::new(
            session.project_content_sha256(),
            session.selected_branch_id().to_owned(),
            diff.source_movie_sha256,
            hunk.start,
        )
    };
    let project_before = state.session.as_ref().unwrap().project().encode().unwrap();
    let generation_before = state.session.as_ref().unwrap().project().edit_generation();
    let dirty_before = state.session.as_ref().unwrap().is_dirty();
    let undo_count_before = state.session.as_ref().unwrap().undo_count();
    std::mem::take(&mut state.timeline_follow_selection);

    state
        .reduce(TasEditorAction::JumpToBranchDiffHunk(action))
        .unwrap();

    let session = state.session.as_ref().unwrap();
    assert_eq!(session.selected_branch_id(), "main");
    assert_eq!(session.cursor(), HUNK_FRAME);
    assert_eq!(
        state.timeline_selection.selected_range(session),
        Some((HUNK_FRAME, HUNK_FRAME + 1))
    );
    assert_eq!(session.project().encode().unwrap(), project_before);
    assert_eq!(session.project().edit_generation(), generation_before);
    assert_eq!(session.is_dirty(), dirty_before);
    assert_eq!(session.undo_count(), undo_count_before);
    assert_eq!(state.take_pending_host_request(), None);
    assert!(std::mem::take(&mut state.timeline_follow_selection));
    assert!(!state.timeline_follow_selection);
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
    let (action, copy_action, source_movie_sha256) = {
        let session = state.session.as_ref().unwrap();
        let diff = wait_for_diff(&mut state.branch_diff_editor, session);
        let hunk = diff.input_hunks.first().unwrap();
        (
            branch_diff_editor::TasBranchDiffJumpAction::new(
                session.project_content_sha256(),
                session.selected_branch_id().to_owned(),
                diff.source_movie_sha256,
                hunk.start,
            ),
            TasEditorAction::InputClipboard(
                input_clipboard::TasInputClipboardAction::copy_constant(
                    session.project_content_sha256(),
                    session.selected_branch_id().to_owned(),
                    diff.source_movie_sha256,
                    hunk.start,
                    hunk.length,
                    hunk.source_input,
                )
                .unwrap(),
            ),
            diff.source_movie_sha256,
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
        .reduce(TasEditorAction::SelectBranch("target".to_owned()))
        .unwrap();
    state
        .reduce(TasEditorAction::ToggleDigital {
            cursor: 0,
            player: 0,
            field: DigitalField::Dpad,
            mask: 1,
        })
        .unwrap();
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .project()
            .branch_movie_sha256("main")
            .unwrap(),
        source_movie_sha256
    );
    state
        .reduce(TasEditorAction::SelectBranch("main".to_owned()))
        .unwrap();
    state
        .reduce(TasEditorAction::SelectTimelineFrame {
            frame: 0,
            extend_selection: false,
        })
        .unwrap();
    std::mem::take(&mut state.timeline_follow_selection);
    let before_stale = state.session.as_ref().unwrap().project().encode().unwrap();
    let jump_error = state
        .reduce(TasEditorAction::JumpToBranchDiffHunk(action))
        .unwrap_err();
    assert!(jump_error.to_string().contains("project changed"));
    assert_eq!(state.session.as_ref().unwrap().cursor(), 0);
    assert_eq!(
        state
            .timeline_selection
            .selected_range(state.session.as_ref().unwrap()),
        Some((0, 1))
    );
    assert!(!state.timeline_follow_selection);
    let copy_error = state.reduce(copy_action).unwrap_err();
    assert!(copy_error.to_string().contains("project changed"));
    assert_eq!(
        state.session.as_ref().unwrap().project().encode().unwrap(),
        before_stale
    );
    assert_eq!(
        state
            .session
            .as_ref()
            .unwrap()
            .project()
            .branch("target")
            .unwrap()
            .input_at(0)
            .players[0]
            .dpad,
        1
    );
}

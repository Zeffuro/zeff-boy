use zeff_emu_common::replay::ReplayEvent;

use super::super::*;
use super::{attach_current_verification, project};

fn input(buttons: u8) -> TasInputFrame {
    TasInputFrame {
        players: [TasControllerInput { buttons, dpad: 0 }; 5],
        ..TasInputFrame::default()
    }
}

fn span(start: u64, length: u64, buttons: u8) -> TasInputSpan {
    TasInputSpan {
        start,
        length,
        input: input(buttons),
    }
}

#[test]
fn billion_frame_neutral_and_constant_patterns_stay_compact() {
    let neutral = TasInputPattern::neutral(MAX_PROJECT_FRAMES).unwrap();
    assert!(neutral.spans().is_empty());
    assert_eq!(neutral.tile_to_length(MAX_PROJECT_FRAMES).unwrap(), neutral);

    let constant = TasInputPattern::constant(1, input(7))
        .unwrap()
        .tile_to_length(MAX_PROJECT_FRAMES)
        .unwrap();
    assert_eq!(constant.length(), MAX_PROJECT_FRAMES);
    assert_eq!(constant.spans(), &[span(0, MAX_PROJECT_FRAMES, 7)]);
}

#[test]
fn branch_extraction_clips_and_rebases_sparse_runs() {
    let mut project = project();
    let branch = &mut project.branches[0];
    branch.input_spans = vec![span(2, 3, 1), span(7, 3, 2)];
    project.validate().unwrap();

    let pattern = project.branch("main").unwrap().input_pattern(3, 6).unwrap();
    assert_eq!(pattern.length(), 6);
    assert_eq!(pattern.spans(), &[span(0, 2, 1), span(4, 2, 2)]);
}

#[test]
fn tiling_preserves_phase_and_clips_the_final_repetition() {
    let pattern = TasInputPattern::new(5, vec![span(1, 2, 1), span(4, 1, 2)]).unwrap();
    let tiled = pattern.tile_to_length(12).unwrap();
    assert_eq!(
        tiled.spans(),
        &[
            span(1, 2, 1),
            span(4, 1, 2),
            span(6, 2, 1),
            span(9, 1, 2),
            span(11, 1, 1),
        ]
    );
}

#[test]
fn tiling_coalesces_identical_runs_across_repetition_boundaries() {
    let pattern =
        TasInputPattern::new(3, vec![span(0, 1, 1), span(1, 1, 2), span(2, 1, 1)]).unwrap();
    let tiled = pattern.tile_to_length(6).unwrap();
    assert_eq!(
        tiled.spans(),
        &[
            span(0, 1, 1),
            span(1, 1, 2),
            span(2, 2, 1),
            span(4, 1, 2),
            span(5, 1, 1),
        ]
    );
}

#[test]
fn branch_extraction_rejects_more_than_the_retained_run_cap() {
    let mut project = project();
    project.branches.truncate(1);
    project.branches[0].frame_count = (MAX_TAS_INPUT_PATTERN_SPANS as u64 + 1) * 2;
    project.branches[0].events.clear();
    project.branches[0].verification = None;
    project.branches[0].input_spans = (0..=MAX_TAS_INPUT_PATTERN_SPANS)
        .map(|index| span(index as u64 * 2, 1, 1))
        .collect();
    project.markers.clear();
    project.annotations.clear();
    project.validate().unwrap();

    let error = project
        .branch("main")
        .unwrap()
        .input_pattern(0, project.branches[0].frame_count)
        .unwrap_err();
    assert!(error.to_string().contains("4096 spans"));
}

#[test]
fn tiling_enforces_candidate_work_and_output_caps() {
    let sparse = TasInputPattern::new(2, vec![span(0, 1, 1)]).unwrap();
    let output_error = sparse.tile_to_length(8_194).unwrap_err();
    assert!(output_error.to_string().contains("output exceeds"));

    let work_error = sparse
        .tile_to_length((MAX_TAS_INPUT_PATTERN_TILE_STEPS as u64 + 1) * 2)
        .unwrap_err();
    assert!(work_error.to_string().contains("candidate runs"));
}

#[test]
fn replacement_splits_and_coalesces_in_one_fixed_length_edit() {
    let mut project = project();
    project.branches[0].input_spans = vec![span(1, 5, 1), span(8, 2, 2)];
    project.validate().unwrap();
    let events = project.branches[0].events.clone();
    let markers = project.markers.clone();
    let annotations = project.annotations.clone();
    let frame_count = project.branches[0].frame_count;
    let pattern = TasInputPattern::new(5, vec![span(0, 2, 2), span(2, 3, 1)]).unwrap();

    project
        .edit_transaction(|edit| edit.replace_input_pattern("main", 3, &pattern))
        .unwrap();

    assert_eq!(
        project.branches[0].input_spans,
        vec![span(1, 2, 1), span(3, 2, 2), span(5, 3, 1), span(8, 2, 2)]
    );
    assert_eq!(project.branches[0].frame_count, frame_count);
    assert_eq!(project.branches[0].events, events);
    assert_eq!(project.markers, markers);
    assert_eq!(project.annotations, annotations);
}

#[test]
fn set_input_range_retains_constant_and_neutral_parity() {
    let mut delegated = project();
    let mut direct = delegated.clone();
    let replacement = input(9);

    delegated
        .edit_transaction(|edit| edit.set_input_range("main", 1, 5, replacement))
        .unwrap();
    let pattern = TasInputPattern::constant(5, replacement).unwrap();
    direct
        .edit_transaction(|edit| edit.replace_input_pattern("main", 1, &pattern))
        .unwrap();
    assert_eq!(delegated, direct);

    delegated
        .edit_transaction(|edit| edit.set_input_range("main", 2, 3, TasInputFrame::default()))
        .unwrap();
    let neutral = TasInputPattern::neutral(3).unwrap();
    direct
        .edit_transaction(|edit| edit.replace_input_pattern("main", 2, &neutral))
        .unwrap();
    assert_eq!(delegated, direct);
}

#[test]
fn exact_noop_preserves_generation_rerecord_verification_events_and_metadata() {
    let mut project = project();
    attach_current_verification(&mut project, "main");
    let before = project.clone();
    let pattern = project.branch("main").unwrap().input_pattern(1, 5).unwrap();

    let outcome = project
        .edit_transaction(|edit| edit.replace_input_pattern("main", 1, &pattern))
        .unwrap();

    assert!(!outcome.changed);
    assert_eq!(outcome.edit_generation, before.edit_generation);
    assert_eq!(outcome.rerecord_count, before.rerecord_count);
    assert_eq!(project, before);
}

#[test]
fn changed_pattern_invalidates_only_target_verification_and_counts_once() {
    let mut project = project();
    attach_current_verification(&mut project, "main");
    attach_current_verification(&mut project, "alternate");
    let alternate_verification = project.branches[1].verification.clone();
    let generation = project.edit_generation;
    let rerecords = project.rerecord_count;
    let events = project.branches[0].events.clone();
    let pattern = TasInputPattern::new(4, vec![span(0, 1, 8), span(3, 1, 9)]).unwrap();

    let outcome = project
        .edit_transaction(|edit| edit.replace_input_pattern("main", 4, &pattern))
        .unwrap();

    assert!(outcome.changed);
    assert_eq!(outcome.edit_generation, generation + 1);
    assert_eq!(outcome.rerecord_count, rerecords + 1);
    assert!(project.branches[0].verification.is_none());
    assert_eq!(project.branches[1].verification, alternate_verification);
    assert_eq!(project.branches[0].events, events);
}

#[test]
fn invalid_patterns_and_replacements_fail_atomically() {
    assert!(TasInputPattern::new(0, Vec::new()).is_err());
    assert!(TasInputPattern::new(2, vec![span(0, 0, 1)]).is_err());
    assert!(
        TasInputPattern::new(
            2,
            vec![TasInputSpan {
                start: 0,
                length: 1,
                input: TasInputFrame::default(),
            }],
        )
        .is_err()
    );
    assert!(TasInputPattern::new(2, vec![span(1, 2, 1)]).is_err());
    assert!(TasInputPattern::new(3, vec![span(1, 2, 1), span(0, 1, 2)]).is_err());
    assert!(TasInputPattern::new(3, vec![span(0, 2, 1), span(1, 1, 2)]).is_err());
    assert!(TasInputPattern::new(2, vec![span(0, 1, 1), span(1, 1, 1)]).is_err());

    let too_many = (0..=MAX_TAS_INPUT_PATTERN_SPANS)
        .map(|index| span(index as u64 * 2, 1, 1))
        .collect();
    assert!(TasInputPattern::new((MAX_TAS_INPUT_PATTERN_SPANS as u64 + 1) * 2, too_many).is_err());

    let mut project = project();
    let before = project.clone();
    let pattern = TasInputPattern::constant(2, input(3)).unwrap();
    assert!(
        project
            .edit_transaction(|edit| edit.replace_input_pattern("main", 11, &pattern))
            .is_err()
    );
    assert_eq!(project, before);
    assert!(
        project
            .edit_transaction(|edit| edit.replace_input_pattern("main", u64::MAX, &pattern))
            .is_err()
    );
    assert_eq!(project, before);
}

#[test]
fn missing_camera_asset_rolls_back_the_complete_pattern_transaction() {
    let mut project = project();
    let before = project.clone();
    let missing = TasDigest([0xEE; 32]);
    let pattern = TasInputPattern::constant(
        1,
        TasInputFrame {
            camera: TasCameraInput::Blob(missing),
            ..TasInputFrame::default()
        },
    )
    .unwrap();

    assert!(
        project
            .edit_transaction(|edit| edit.replace_input_pattern("main", 0, &pattern))
            .is_err()
    );
    assert_eq!(project, before);
}

#[test]
fn pattern_replacement_does_not_move_frame_boundary_events() {
    let mut project = project();
    project.branches[0].events = vec![ReplayEvent::FdsDiskSide { frame: 6, side: 2 }];
    project.validate().unwrap();
    let pattern = TasInputPattern::constant(3, input(4)).unwrap();

    project
        .edit_transaction(|edit| edit.replace_input_pattern("main", 5, &pattern))
        .unwrap();

    assert_eq!(
        project.branches[0].events,
        vec![ReplayEvent::FdsDiskSide { frame: 6, side: 2 }]
    );
}

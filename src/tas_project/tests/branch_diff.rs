use zeff_emu_common::{
    media::{MediaEvent, MediaSlotId},
    replay::ReplayEvent,
};

use super::*;

fn input(buttons: u8) -> TasInputFrame {
    let mut input = TasInputFrame::default();
    input.players[0].buttons = buttons;
    input
}

fn two_branch_project(frame_count: u64) -> TasProject {
    let mut project = project();
    project.branches.truncate(1);
    project.branches[0].frame_count = frame_count;
    project.branches[0].input_spans.clear();
    project.branches[0].events.clear();
    project.branches[0].verification = None;
    project.markers.clear();
    project.annotations.clear();
    let mut target = project.branches[0].clone();
    target.id = "target".to_owned();
    target.name = "Target".to_owned();
    target.parent = None;
    project.branches.push(target);
    project.validate().unwrap();
    project
}

fn canonical(mut events: Vec<ReplayEvent>) -> Vec<ReplayEvent> {
    events.sort_by(ReplayEvent::canonical_cmp);
    events
}

#[test]
fn sparse_input_diff_uses_maximal_common_timeline_ranges_and_separate_tail() {
    let mut project = two_branch_project(10);
    project.branches[0].input_spans = vec![
        TasInputSpan {
            start: 1,
            length: 4,
            input: input(1),
        },
        TasInputSpan {
            start: 7,
            length: 3,
            input: input(2),
        },
    ];
    project.branches[1].frame_count = 12;
    project.branches[1].input_spans = vec![
        TasInputSpan {
            start: 1,
            length: 2,
            input: input(1),
        },
        TasInputSpan {
            start: 3,
            length: 2,
            input: input(3),
        },
        TasInputSpan {
            start: 8,
            length: 4,
            input: input(4),
        },
    ];
    project.validate().unwrap();

    let diff = project
        .diff_branches("main", "target", TasBranchDiffLimits::default())
        .unwrap();

    assert_eq!(
        diff.input_hunks,
        vec![
            TasInputDiffHunk {
                start: 3,
                length: 2,
                source_input: input(1),
                target_input: input(3),
            },
            TasInputDiffHunk {
                start: 7,
                length: 1,
                source_input: input(2),
                target_input: TasInputFrame::default(),
            },
            TasInputDiffHunk {
                start: 8,
                length: 2,
                source_input: input(2),
                target_input: input(4),
            },
        ]
    );
    assert_eq!(
        diff.timeline_tail,
        Some(TasTimelineTailDiff {
            longer_side: TasBranchDiffSide::Target,
            start: 10,
            length: 2,
        })
    );
    assert!(diff.input_hunks.iter().all(|hunk| hunk.start < 10));
    assert_eq!(
        diff.source_movie_sha256,
        project.branch_movie_sha256("main").unwrap()
    );
    assert_eq!(
        diff.target_movie_sha256,
        project.branch_movie_sha256("target").unwrap()
    );
}

#[test]
fn input_hunks_preserve_every_materialized_channel_bit_exactly() {
    let mut project = two_branch_project(2);
    let camera = *project.assets.keys().next().unwrap();
    let source_input = TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 1,
                dpad: 2,
            },
            TasControllerInput {
                buttons: 3,
                dpad: 4,
            },
            TasControllerInput {
                buttons: 5,
                dpad: 6,
            },
            TasControllerInput {
                buttons: 7,
                dpad: 8,
            },
            TasControllerInput {
                buttons: 9,
                dpad: 10,
            },
        ],
        coleco: [TasColecoControllerInput::default(); 2],
        zapper: TasZapperInput {
            enabled: true,
            trigger: true,
            hit: true,
            screen_pos: Some([0, u16::MAX]),
        },
        tilt_x_bits: 0x7FC0_0123,
        tilt_y_bits: 0x8000_0000,
        camera: TasCameraInput::Blob(camera),
    };
    project.branches[0].input_spans = vec![TasInputSpan {
        start: 0,
        length: 1,
        input: source_input,
    }];
    project.validate().unwrap();

    let diff = project
        .diff_branches("main", "target", TasBranchDiffLimits::default())
        .unwrap();

    assert_eq!(
        diff.input_hunks,
        vec![TasInputDiffHunk {
            start: 0,
            length: 1,
            source_input,
            target_input: TasInputFrame::default(),
        }]
    );
}

#[test]
fn billion_frame_length_difference_never_turns_an_absent_tail_into_neutral_input() {
    let mut project = two_branch_project(1_000_000_000);
    project.branches[1].frame_count = 2;
    project.validate().unwrap();

    let diff = project
        .diff_branches("main", "target", TasBranchDiffLimits::default())
        .unwrap();

    assert!(diff.input_hunks.is_empty());
    assert_eq!(
        diff.timeline_tail,
        Some(TasTimelineTailDiff {
            longer_side: TasBranchDiffSide::Source,
            start: 2,
            length: 999_999_998,
        })
    );
}

#[test]
fn canonical_equal_key_event_runs_return_only_exact_index_ranges() {
    let mut project = two_branch_project(5);
    let media = ReplayEvent::Media {
        frame: 1,
        sequence: 0,
        event: MediaEvent::Eject {
            slot: MediaSlotId::new("cart"),
        },
    };
    project.branches[0].events = canonical(vec![
        ReplayEvent::FdsDiskSide { frame: 1, side: 0 },
        media.clone(),
        ReplayEvent::FdsDiskSide { frame: 2, side: 0 },
        ReplayEvent::FdsDiskSide { frame: 4, side: 0 },
    ]);
    project.branches[1].events = canonical(vec![
        ReplayEvent::FdsDiskSide { frame: 1, side: 1 },
        media,
        ReplayEvent::FdsDiskSide { frame: 3, side: 0 },
        ReplayEvent::FdsDiskSide { frame: 4, side: 0 },
    ]);
    project.validate().unwrap();

    let diff = project
        .diff_branches("main", "target", TasBranchDiffLimits::default())
        .unwrap();

    assert_eq!(
        diff.event_hunks,
        vec![
            TasEventDiffHunk {
                kind: TasEventDiffKind::Changed,
                source_event_indices: 0..2,
                target_event_indices: 0..2,
                first_frame: 1,
                last_frame: 1,
            },
            TasEventDiffHunk {
                kind: TasEventDiffKind::SourceOnly,
                source_event_indices: 2..3,
                target_event_indices: 2..2,
                first_frame: 2,
                last_frame: 2,
            },
            TasEventDiffHunk {
                kind: TasEventDiffKind::TargetOnly,
                source_event_indices: 3..3,
                target_event_indices: 2..3,
                first_frame: 3,
                last_frame: 3,
            },
        ]
    );
}

#[test]
fn adjacent_event_groups_of_the_same_kind_coalesce_for_copyable_ranges() {
    let mut project = two_branch_project(5);
    project.branches[0].events = vec![
        ReplayEvent::FdsDiskSide { frame: 1, side: 0 },
        ReplayEvent::FdsDiskSide { frame: 2, side: 1 },
        ReplayEvent::FdsDiskSide { frame: 3, side: 0 },
    ];
    project.validate().unwrap();

    let diff = project
        .diff_branches("main", "target", TasBranchDiffLimits::default())
        .unwrap();

    assert_eq!(
        diff.event_hunks,
        vec![TasEventDiffHunk {
            kind: TasEventDiffKind::SourceOnly,
            source_event_indices: 0..3,
            target_event_indices: 0..0,
            first_frame: 1,
            last_frame: 3,
        }]
    );
}

#[test]
fn hunk_retention_is_bounded_while_omitted_counts_remain_exact() {
    let mut project = two_branch_project(8);
    project.branches[1].input_spans = (0..8)
        .step_by(2)
        .map(|start| TasInputSpan {
            start,
            length: 1,
            input: input(1),
        })
        .collect();
    project.branches[0].events = (0..8)
        .map(|frame| ReplayEvent::FdsDiskSide { frame, side: 0 })
        .collect();
    project.branches[1].events = (0..8)
        .map(|frame| ReplayEvent::FdsDiskSide {
            frame,
            side: u8::from(frame % 2 == 0),
        })
        .collect();
    project.validate().unwrap();
    let limits = TasBranchDiffLimits {
        max_input_hunks: 1,
        max_event_hunks: 1,
        ..TasBranchDiffLimits::default()
    };

    let diff = project.diff_branches("main", "target", limits).unwrap();

    assert_eq!(diff.input_hunks.len(), 1);
    assert_eq!(diff.omitted_input_hunks, 3);
    assert_eq!(diff.event_hunks.len(), 1);
    assert_eq!(diff.omitted_event_hunks, 3);
    assert!(diff.is_truncated());
}

#[test]
fn configured_and_hard_scan_limits_fail_before_unbounded_work() {
    let mut project = two_branch_project(6);
    project.branches[0].input_spans = vec![TasInputSpan {
        start: 0,
        length: 1,
        input: input(1),
    }];
    project.branches[1].input_spans = vec![TasInputSpan {
        start: 2,
        length: 1,
        input: input(2),
    }];
    project.branches[0].events = vec![ReplayEvent::FdsDiskSide { frame: 1, side: 0 }];
    project.branches[1].events = vec![ReplayEvent::FdsDiskSide { frame: 2, side: 0 }];
    project.validate().unwrap();

    let input_error = project
        .diff_branches(
            "main",
            "target",
            TasBranchDiffLimits {
                max_input_spans_scanned: 1,
                ..TasBranchDiffLimits::default()
            },
        )
        .unwrap_err();
    assert!(
        input_error
            .to_string()
            .contains("requires scanning 2 input spans")
    );

    let event_error = project
        .diff_branches(
            "main",
            "target",
            TasBranchDiffLimits {
                max_events_scanned: 1,
                ..TasBranchDiffLimits::default()
            },
        )
        .unwrap_err();
    assert!(
        event_error
            .to_string()
            .contains("requires scanning 2 events")
    );

    let hard_error = project
        .diff_branches(
            "main",
            "target",
            TasBranchDiffLimits {
                max_input_hunks: MAX_BRANCH_DIFF_RETAINED_HUNKS + 1,
                ..TasBranchDiffLimits::default()
            },
        )
        .unwrap_err();
    assert!(hard_error.to_string().contains("hard maximum"));
}

#[test]
fn presentation_and_provenance_fields_do_not_enter_the_movie_diff() {
    let mut project = two_branch_project(4);
    project.branches[1].name = "Presentation-only rename".to_owned();
    project.branches[1].comment = "Branch note".to_owned();
    project.markers.push(TasMarker {
        id: "target-marker".to_owned(),
        branch_id: "target".to_owned(),
        cursor: 2,
        name: "Marker".to_owned(),
    });
    project.annotations.push(TasAnnotation {
        id: "target-note".to_owned(),
        branch_id: "target".to_owned(),
        start: 1,
        length: 1,
        kind: "note".to_owned(),
        text: "Presentation".to_owned(),
    });
    let target_movie_sha256 = project.branch_movie_sha256("target").unwrap();
    project.branches[1].verification = Some(TasVerificationProvenance {
        branch_movie_sha256: target_movie_sha256,
        checkpoints: vec![TasVerificationCheckpoint {
            cursor: 2,
            state_sha256: TasDigest([0xD1; 32]),
        }],
        final_state_sha256: Some(TasDigest([0xD2; 32])),
    });
    project.validate().unwrap();

    let diff = project
        .diff_branches("main", "target", TasBranchDiffLimits::default())
        .unwrap();

    assert!(diff.is_identical());
}

#[test]
fn identical_and_unknown_branch_requests_are_safe() {
    let project = two_branch_project(0);
    let identical = project
        .diff_branches("main", "main", TasBranchDiffLimits::default())
        .unwrap();
    assert!(identical.is_identical());

    let error = project
        .diff_branches("missing", "missing", TasBranchDiffLimits::default())
        .unwrap_err();
    assert!(error.to_string().contains("unknown TAS branch"));
}

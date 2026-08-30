use zeff_emu_common::media::{MediaEvent, MediaSlotId};
use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkAction, ReplayGameBoyLinkEvent, ReplayGameBoyLinkState,
    ReplayWonderSwanLinkEvent,
};

use super::super::*;
use super::{attach_current_verification, project};

fn input(buttons: u8) -> TasInputFrame {
    TasInputFrame {
        players: [TasControllerInput { buttons, dpad: 0 }; 5],
        ..TasInputFrame::default()
    }
}

fn idle_gb_link_state() -> ReplayGameBoyLinkState {
    ReplayGameBoyLinkState {
        peer_present: false,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 0,
    }
}

fn event_variant(index: usize, frame: u64) -> ReplayEvent {
    match index {
        0 => ReplayEvent::FdsDiskSide { frame, side: 1 },
        1 => ReplayEvent::Media {
            frame,
            sequence: 7,
            event: MediaEvent::Eject {
                slot: MediaSlotId::new("disk"),
            },
        },
        2 => ReplayEvent::GameBoyLink {
            frame,
            tick: 123,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: 9,
                clock_period_t_cycles: 512,
                out_byte: 0xA5,
                serial_generation: 4,
            },
        },
        3 => ReplayEvent::GameBoyLinkState {
            frame,
            state: idle_gb_link_state(),
        },
        4 => ReplayEvent::GameBoyLinkStateAtTick {
            frame,
            tick: 456,
            state: idle_gb_link_state(),
        },
        5 => ReplayEvent::WonderSwanLink {
            frame,
            session_cycle: 789,
            event: ReplayWonderSwanLinkEvent::RemoteByte {
                generation: 3,
                baud_bps: 9_600,
                byte: 0x5A,
            },
        },
        _ => unreachable!("test enumerates every current ReplayEvent variant"),
    }
}

#[test]
fn insertion_rebases_every_branch_local_timeline_domain_at_exact_boundaries() {
    let mut project = project();
    let source_replay = TasDigest([0xA1; 32]);
    project.source_replay_sha256 = Some(source_replay);
    let original_parent_hash = project.branches[1]
        .parent
        .as_ref()
        .unwrap()
        .branch_movie_sha256;
    project.branches[1].input_spans = vec![
        TasInputSpan {
            start: 0,
            length: 2,
            input: input(1),
        },
        TasInputSpan {
            start: 2,
            length: 4,
            input: input(2),
        },
        TasInputSpan {
            start: 7,
            length: 2,
            input: input(3),
        },
    ];
    project.markers.extend([
        TasMarker {
            id: "alt-before".to_owned(),
            branch_id: "alternate".to_owned(),
            cursor: 3,
            name: "Before".to_owned(),
        },
        TasMarker {
            id: "alt-at".to_owned(),
            branch_id: "alternate".to_owned(),
            cursor: 4,
            name: "At".to_owned(),
        },
        TasMarker {
            id: "alt-end".to_owned(),
            branch_id: "alternate".to_owned(),
            cursor: 12,
            name: "End".to_owned(),
        },
    ]);
    project.annotations.extend([
        annotation("insert-before", 0, 2),
        annotation("insert-ends-at", 1, 3),
        annotation("insert-starts-at", 4, 2),
        annotation("insert-contains", 3, 3),
        annotation("insert-after", 7, 2),
    ]);
    attach_current_verification(&mut project, "alternate");
    project.branches[1]
        .verification
        .as_mut()
        .unwrap()
        .checkpoints = vec![TasVerificationCheckpoint {
        cursor: 12,
        state_sha256: TasDigest([0x91; 32]),
    }];
    let main_marker = project.markers[0].clone();
    let main_annotation = project.annotations[0].clone();

    let outcome = project
        .edit_transaction(|edit| edit.insert_frames("alternate", 4, 2))
        .unwrap();

    assert_eq!(outcome.edit_generation, 4);
    assert_eq!(outcome.rerecord_count, 3);
    assert_eq!(
        outcome.branch_impacts,
        vec![TasBranchEditImpact {
            branch_id: "alternate".to_owned(),
            kind: TasBranchEditImpactKind::Modified { earliest_cursor: 4 },
        }]
    );
    let branch = project.branch("alternate").unwrap();
    assert_eq!(branch.frame_count, 14);
    assert_eq!(
        branch.input_spans,
        vec![
            TasInputSpan {
                start: 0,
                length: 2,
                input: input(1),
            },
            TasInputSpan {
                start: 2,
                length: 2,
                input: input(2),
            },
            TasInputSpan {
                start: 6,
                length: 2,
                input: input(2),
            },
            TasInputSpan {
                start: 9,
                length: 2,
                input: input(3),
            },
        ]
    );
    assert_eq!(branch.parent.as_ref().unwrap().fork_cursor, 4);
    assert_eq!(
        branch.parent.as_ref().unwrap().branch_movie_sha256,
        original_parent_hash
    );
    assert!(branch.verification.is_none());
    assert_eq!(marker_cursor(&project, "alt-before"), 3);
    assert_eq!(marker_cursor(&project, "alt-at"), 6);
    assert_eq!(marker_cursor(&project, "alt-end"), 14);
    assert_eq!(annotation_range(&project, "insert-before"), (0, 2));
    assert_eq!(annotation_range(&project, "insert-ends-at"), (1, 3));
    assert_eq!(annotation_range(&project, "insert-starts-at"), (6, 2));
    assert_eq!(annotation_range(&project, "insert-contains"), (3, 5));
    assert_eq!(annotation_range(&project, "insert-after"), (9, 2));
    assert_eq!(project.markers[0], main_marker);
    assert_eq!(project.annotations[0], main_annotation);
    assert_eq!(project.source_replay_sha256, Some(source_replay));
    project.validate().unwrap();
}

#[test]
fn deletion_retains_only_surviving_spans_and_collapses_cursors() {
    let mut project = project();
    let original_parent_hash = project.branches[1]
        .parent
        .as_ref()
        .unwrap()
        .branch_movie_sha256;
    project.branches[1].input_spans = vec![
        TasInputSpan {
            start: 2,
            length: 3,
            input: input(1),
        },
        TasInputSpan {
            start: 7,
            length: 2,
            input: input(1),
        },
        TasInputSpan {
            start: 9,
            length: 2,
            input: input(2),
        },
    ];
    project.markers.extend([
        marker("delete-before", 2),
        marker("delete-at-start", 3),
        marker("delete-inside", 5),
        marker("delete-at-end", 7),
        marker("delete-after", 9),
        marker("delete-end", 12),
    ]);
    project.annotations.extend([
        annotation("delete-before-ann", 0, 2),
        annotation("delete-ends-at", 1, 2),
        annotation("delete-inside-ann", 4, 2),
        annotation("delete-left-overlap", 2, 3),
        annotation("delete-right-overlap", 5, 4),
        annotation("delete-contains", 2, 7),
        annotation("delete-starts-at-end", 7, 2),
        annotation("delete-after-ann", 9, 2),
        annotation("delete-exact", 3, 4),
    ]);
    attach_current_verification(&mut project, "alternate");
    project.branches[1]
        .verification
        .as_mut()
        .unwrap()
        .checkpoints = vec![TasVerificationCheckpoint {
        cursor: 12,
        state_sha256: TasDigest([0x92; 32]),
    }];

    let outcome = project
        .edit_transaction(|edit| edit.delete_frames("alternate", 3, 4))
        .unwrap();

    assert_eq!(outcome.edit_generation, 4);
    assert_eq!(outcome.rerecord_count, 3);
    assert_eq!(
        outcome.branch_impacts[0].kind,
        TasBranchEditImpactKind::Modified { earliest_cursor: 3 }
    );
    let branch = project.branch("alternate").unwrap();
    assert_eq!(branch.frame_count, 8);
    assert_eq!(
        branch.input_spans,
        vec![
            TasInputSpan {
                start: 2,
                length: 3,
                input: input(1),
            },
            TasInputSpan {
                start: 5,
                length: 2,
                input: input(2),
            },
        ]
    );
    assert_eq!(branch.parent.as_ref().unwrap().fork_cursor, 4);
    assert_eq!(
        branch.parent.as_ref().unwrap().branch_movie_sha256,
        original_parent_hash
    );
    assert!(branch.verification.is_none());
    assert_eq!(marker_cursor(&project, "delete-before"), 2);
    assert_eq!(marker_cursor(&project, "delete-at-start"), 3);
    assert_eq!(marker_cursor(&project, "delete-inside"), 3);
    assert_eq!(marker_cursor(&project, "delete-at-end"), 3);
    assert_eq!(marker_cursor(&project, "delete-after"), 5);
    assert_eq!(marker_cursor(&project, "delete-end"), 8);
    assert_eq!(annotation_range(&project, "delete-before-ann"), (0, 2));
    assert_eq!(annotation_range(&project, "delete-ends-at"), (1, 2));
    assert!(annotation_by_id(&project, "delete-inside-ann").is_none());
    assert_eq!(annotation_range(&project, "delete-left-overlap"), (2, 1));
    assert_eq!(annotation_range(&project, "delete-right-overlap"), (3, 2));
    assert_eq!(annotation_range(&project, "delete-contains"), (2, 3));
    assert_eq!(annotation_range(&project, "delete-starts-at-end"), (3, 2));
    assert_eq!(annotation_range(&project, "delete-after-ann"), (5, 2));
    assert!(annotation_by_id(&project, "delete-exact").is_none());
    project.validate().unwrap();
}

#[test]
fn every_replay_event_variant_uses_the_same_insert_and_delete_boundaries() {
    for index in 0..6 {
        let mut inserted = project();
        inserted.branches[0].events = vec![event_variant(index, 4)];
        inserted.validate().unwrap();
        let outcome = inserted
            .edit_transaction(|edit| edit.insert_frames("main", 4, 2))
            .unwrap();
        assert_eq!(inserted.branches[0].events[0].frame(), 6, "variant {index}");
        assert_eq!(
            outcome.branch_impacts[0].kind,
            TasBranchEditImpactKind::Modified { earliest_cursor: 4 },
            "variant {index}"
        );

        let mut deleted_at_start = project();
        deleted_at_start.branches[0].events = vec![event_variant(index, 4)];
        deleted_at_start.validate().unwrap();
        deleted_at_start
            .edit_transaction(|edit| edit.delete_frames("main", 4, 2))
            .unwrap();
        assert!(
            deleted_at_start.branches[0].events.is_empty(),
            "variant {index}"
        );

        let mut deleted_at_end = project();
        deleted_at_end.branches[0].events = vec![event_variant(index, 6)];
        deleted_at_end.validate().unwrap();
        deleted_at_end
            .edit_transaction(|edit| edit.delete_frames("main", 4, 2))
            .unwrap();
        assert_eq!(
            deleted_at_end.branches[0].events[0].frame(),
            4,
            "variant {index}"
        );
    }
}

#[test]
fn insert_then_delete_is_an_exact_transactional_inverse_at_every_cursor() {
    let mut base = project();
    base.branches[1].events = (0..6)
        .map(|index| event_variant(index, index as u64))
        .collect();
    base.markers.push(marker("inverse-marker", 4));
    base.annotations
        .push(annotation("inverse-annotation", 2, 5));
    attach_current_verification(&mut base, "alternate");
    base.validate().unwrap();

    for cursor in 0..=base.branch("alternate").unwrap().frame_count {
        let mut candidate = base.clone();
        let before_bytes = candidate.encode().unwrap();
        let outcome = candidate
            .edit_transaction(|edit| {
                edit.insert_frames("alternate", cursor, 3)?;
                edit.delete_frames("alternate", cursor, 3)
            })
            .unwrap();
        assert!(!outcome.changed, "cursor {cursor}");
        assert!(outcome.branch_impacts.is_empty(), "cursor {cursor}");
        assert_eq!(candidate, base, "cursor {cursor}");
        assert_eq!(candidate.encode().unwrap(), before_bytes, "cursor {cursor}");
    }
}

#[test]
fn timeline_impact_starts_at_the_requested_cursor_even_for_neutral_frames() {
    let mut inserted = project();
    inserted.branches[0].input_spans.clear();
    inserted.branches[0].events.clear();
    let outcome = inserted
        .edit_transaction(|edit| edit.insert_frames("main", 2, 1))
        .unwrap();
    assert_eq!(
        outcome.branch_impacts[0].kind,
        TasBranchEditImpactKind::Modified { earliest_cursor: 2 }
    );

    let mut deleted = project();
    deleted.branches[0].input_spans.clear();
    deleted.branches[0].events.clear();
    let outcome = deleted
        .edit_transaction(|edit| edit.delete_frames("main", 2, 1))
        .unwrap();
    assert_eq!(
        outcome.branch_impacts[0].kind,
        TasBranchEditImpactKind::Modified { earliest_cursor: 2 }
    );
}

#[test]
fn editing_a_parent_does_not_rewrite_a_child_snapshot_origin() {
    let mut project = project();
    let origin = project.branches[1].parent.clone();
    project
        .edit_transaction(|edit| edit.insert_frames("main", 0, 2))
        .unwrap();
    assert_eq!(project.branches[1].parent, origin);
}

#[test]
fn editing_a_child_does_not_rewrite_its_snapshot_origin() {
    let base = project();
    let origin = base.branches[1].parent.clone();

    let mut inserted = base.clone();
    inserted
        .edit_transaction(|edit| edit.insert_frames("alternate", 0, 2))
        .unwrap();
    assert_eq!(inserted.branches[1].parent, origin);

    let mut deleted = base;
    deleted
        .edit_transaction(|edit| edit.delete_frames("alternate", 0, 2))
        .unwrap();
    assert_eq!(deleted.branches[1].parent, origin);
}

#[test]
fn timeline_failures_leave_the_project_and_encoded_bytes_exactly_unchanged() {
    assert_atomic_failure(project(), |edit| edit.insert_frames("main", 0, 0));
    assert_atomic_failure(project(), |edit| edit.insert_frames("main", 13, 1));
    assert_atomic_failure(project(), |edit| edit.insert_frames("main", 0, u64::MAX));
    assert_atomic_failure(project(), |edit| edit.delete_frames("main", 0, 0));
    assert_atomic_failure(project(), |edit| edit.delete_frames("main", 13, 1));
    assert_atomic_failure(project(), |edit| edit.delete_frames("main", u64::MAX, 2));

    let mut at_limit = project();
    at_limit.branches[0].frame_count = MAX_PROJECT_FRAMES;
    assert_atomic_failure(at_limit, |edit| {
        edit.insert_frames("main", MAX_PROJECT_FRAMES, 1)
    });

    let mut generation_overflow = project();
    generation_overflow.edit_generation = u64::MAX;
    assert_atomic_failure(generation_overflow, |edit| edit.insert_frames("main", 0, 1));

    let mut rerecord_overflow = project();
    rerecord_overflow.rerecord_count = u64::MAX;
    assert_atomic_failure(rerecord_overflow, |edit| edit.delete_frames("main", 0, 1));

    let mut invalid_rebase = project();
    let action = ReplayGameBoyLinkAction {
        out_byte: 0x3C,
        clock_period_t_cycles: 512,
        serial_generation: 8,
    };
    invalid_rebase.replay_start.game_boy_link_state = Some(ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: Some(action.out_byte),
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: Some(action),
        pending_passive_completion: None,
        serial_generation: action.serial_generation,
    });
    let matching_event = ReplayEvent::GameBoyLink {
        frame: 4,
        tick: 123,
        event: ReplayGameBoyLinkEvent::LocalMasterStart {
            transfer_id: 10,
            clock_period_t_cycles: action.clock_period_t_cycles,
            out_byte: action.out_byte,
            serial_generation: action.serial_generation,
        },
    };
    invalid_rebase.branches[0].events = vec![matching_event.clone()];
    invalid_rebase.branches[1].events = vec![matching_event];
    invalid_rebase.validate().unwrap();
    assert_atomic_failure(invalid_rebase, |edit| edit.delete_frames("main", 4, 1));
}

fn marker(id: &str, cursor: u64) -> TasMarker {
    TasMarker {
        id: id.to_owned(),
        branch_id: "alternate".to_owned(),
        cursor,
        name: id.to_owned(),
    }
}

fn annotation(id: &str, start: u64, length: u64) -> TasAnnotation {
    TasAnnotation {
        id: id.to_owned(),
        branch_id: "alternate".to_owned(),
        start,
        length,
        kind: "note".to_owned(),
        text: id.to_owned(),
    }
}

fn marker_cursor(project: &TasProject, id: &str) -> u64 {
    project
        .markers
        .iter()
        .find(|marker| marker.id == id)
        .unwrap()
        .cursor
}

fn annotation_by_id<'a>(project: &'a TasProject, id: &str) -> Option<&'a TasAnnotation> {
    project
        .annotations
        .iter()
        .find(|annotation| annotation.id == id)
}

fn annotation_range(project: &TasProject, id: &str) -> (u64, u64) {
    let annotation = annotation_by_id(project, id).unwrap();
    (annotation.start, annotation.length)
}

fn assert_atomic_failure(
    mut project: TasProject,
    edit: impl FnOnce(&mut TasProjectEdit<'_>) -> anyhow::Result<()>,
) {
    let before = project.clone();
    let before_bytes = project.encode().unwrap();
    assert!(project.edit_transaction(edit).is_err());
    assert_eq!(project, before);
    assert_eq!(project.encode().unwrap(), before_bytes);
}

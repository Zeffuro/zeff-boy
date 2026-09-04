use zeff_emu_common::replay::ReplayEvent;

use super::super::*;
use super::{attach_current_verification, project};

#[test]
fn edit_transaction_normalizes_input_and_invalidates_only_changed_branch() {
    let mut project = project();
    attach_current_verification(&mut project, "main");
    attach_current_verification(&mut project, "alternate");
    let alternate_verification = project.branches[1].verification.clone();
    let replacement = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0x40,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };

    let outcome = project
        .edit_transaction(|edit| edit.set_input_range("main", 3, 2, replacement))
        .unwrap();

    assert_eq!(outcome.edit_generation, 4);
    assert_eq!(outcome.rerecord_count, 3);
    assert_eq!(
        outcome.branch_impacts,
        vec![TasBranchEditImpact {
            branch_id: "main".to_owned(),
            kind: TasBranchEditImpactKind::Modified { earliest_cursor: 3 },
        }]
    );
    assert_eq!(project.branches[0].input_spans.len(), 2);
    assert_eq!(project.branches[0].input_spans[0].start, 2);
    assert_eq!(project.branches[0].input_spans[0].length, 1);
    assert_eq!(project.branches[0].input_spans[1].start, 3);
    assert_eq!(project.branches[0].input_spans[1].length, 2);
    assert!(project.branches[0].verification.is_none());
    assert_eq!(project.branches[1].verification, alternate_verification);
    assert_eq!(project.markers[0].cursor, 10);
    assert_eq!(project.annotations[0].start, 3);
}

#[test]
fn edit_transaction_merges_spans_and_omits_neutral_input() {
    let mut project = project();
    let original_input = project.branches[0].input_spans[0].input;

    let outcome = project
        .edit_transaction(|edit| edit.set_input_range("main", 4, 2, original_input))
        .unwrap();
    assert_eq!(
        outcome.branch_impacts[0].kind,
        TasBranchEditImpactKind::Modified { earliest_cursor: 4 }
    );
    assert_eq!(
        project.branches[0].input_spans,
        vec![TasInputSpan {
            start: 2,
            length: 4,
            input: original_input,
        }]
    );

    project
        .edit_transaction(|edit| edit.set_input_range("main", 2, 4, TasInputFrame::default()))
        .unwrap();
    assert!(project.branches[0].input_spans.is_empty());
}

#[test]
fn edit_transaction_presentation_changes_generation_but_not_movie_provenance() {
    let mut project = project();
    attach_current_verification(&mut project, "main");
    let movie_hash = project.branch_movie_sha256("main").unwrap();
    let input = project.branches[0].input_spans[0].input;
    let events = project.branches[0].events.clone();
    let verification = project.branches[0].verification.clone();

    let outcome = project
        .edit_transaction(|edit| {
            edit.rename_branch("main", "Renamed")?;
            edit.set_branch_comment("main", "new notes")?;
            edit.set_project_comment("new project notes");
            edit.set_active_branch("alternate")?;
            edit.set_input_range("main", 2, 2, input)?;
            edit.replace_branch_events("main", events)?;
            Ok(())
        })
        .unwrap();

    assert!(outcome.changed);
    assert_eq!(outcome.edit_generation, 4);
    assert_eq!(outcome.rerecord_count, 2);
    assert!(outcome.branch_impacts.is_empty());
    assert_eq!(project.branch_movie_sha256("main").unwrap(), movie_hash);
    assert_eq!(project.branches[0].verification, verification);
    assert_eq!(project.active_branch_id, "alternate");

    let before_reverted_edit = project.clone();
    let changed = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0x20,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };
    let outcome = project
        .edit_transaction(|edit| {
            edit.set_input_range("main", 0, 1, changed)?;
            edit.set_input_range("main", 0, 1, TasInputFrame::default())?;
            Ok(())
        })
        .unwrap();
    assert!(!outcome.changed);
    assert!(outcome.branch_impacts.is_empty());
    assert_eq!(project, before_reverted_edit);
}

#[test]
fn rename_branch_is_a_presentation_only_transaction() {
    let mut project = project();
    attach_current_verification(&mut project, "main");
    let movie_hash = project.branch_movie_sha256("main").unwrap();
    let verification = project.branches[0].verification.clone();
    let rerecord_count = project.rerecord_count;

    let outcome = project
        .edit_transaction(|edit| edit.rename_branch("main", "Any% route"))
        .unwrap();

    assert!(outcome.changed);
    assert_eq!(project.branches[0].name, "Any% route");
    assert_eq!(project.branch_movie_sha256("main").unwrap(), movie_hash);
    assert_eq!(project.branches[0].verification, verification);
    assert_eq!(project.rerecord_count, rerecord_count);
    assert!(outcome.branch_impacts.is_empty());
}

#[test]
fn fork_transaction_captures_an_independent_full_snapshot() {
    let mut project = project();
    attach_current_verification(&mut project, "main");
    let parent_hash = project.branch_movie_sha256("main").unwrap();
    let parent_timeline = project.branches[0].clone();

    let outcome = project
        .edit_transaction(|edit| {
            edit.fork_branch("main", 6, "route-b", "Route B")?;
            edit.set_active_branch("route-b")?;
            Ok(())
        })
        .unwrap();

    assert_eq!(outcome.edit_generation, 4);
    assert_eq!(outcome.rerecord_count, 2);
    assert_eq!(
        outcome.branch_impacts,
        vec![TasBranchEditImpact {
            branch_id: "route-b".to_owned(),
            kind: TasBranchEditImpactKind::Created { fork_cursor: 6 },
        }]
    );
    let child = project.branch("route-b").unwrap();
    assert_eq!(child.frame_count, parent_timeline.frame_count);
    assert_eq!(child.input_spans, parent_timeline.input_spans);
    assert_eq!(child.events, parent_timeline.events);
    assert!(child.verification.is_none());
    assert_eq!(
        child.parent.as_ref().unwrap().branch_movie_sha256,
        parent_hash
    );
    assert_eq!(project.branch_movie_sha256("route-b").unwrap(), parent_hash);

    let child_snapshot = child.clone();
    let changed_parent_input = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0x08,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };
    project
        .edit_transaction(|edit| edit.set_input_range("main", 0, 1, changed_parent_input))
        .unwrap();
    assert_ne!(project.branch_movie_sha256("main").unwrap(), parent_hash);
    assert_eq!(project.branch("route-b").unwrap(), &child_snapshot);
    assert_eq!(
        TasProject::decode(&project.encode().unwrap()).unwrap(),
        project
    );
}

#[test]
fn mixed_edit_transaction_bumps_each_counter_at_most_once() {
    let mut project = project();
    let input_a = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0x10,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };
    let input_b = TasInputFrame {
        players: [TasControllerInput {
            buttons: 0x20,
            dpad: 0,
        }; 5],
        ..TasInputFrame::default()
    };

    project
        .edit_transaction(|edit| {
            edit.set_input_range("main", 0, 1, input_a)?;
            edit.set_input_range("main", 1, 1, input_b)?;
            edit.set_input_range("alternate", 0, 1, input_a)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(project.edit_generation, 4);
    assert_eq!(project.rerecord_count, 3);

    let outcome = project
        .edit_transaction(|edit| {
            edit.fork_branch("main", 4, "new-route", "New Route")?;
            edit.set_input_range("new-route", 5, 1, input_b)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(outcome.edit_generation, 5);
    assert_eq!(outcome.rerecord_count, 4);
}

#[test]
fn edit_transaction_canonicalizes_events_and_reports_the_first_changed_cursor() {
    let mut project = project();
    let outcome = project
        .edit_transaction(|edit| {
            edit.replace_branch_events(
                "main",
                vec![
                    ReplayEvent::FdsDiskSide { frame: 5, side: 0 },
                    ReplayEvent::FdsDiskSide { frame: 1, side: 1 },
                ],
            )
        })
        .unwrap();

    assert_eq!(project.branches[0].events[0].frame(), 1);
    assert_eq!(project.branches[0].events[1].frame(), 5);
    assert_eq!(
        outcome.branch_impacts[0].kind,
        TasBranchEditImpactKind::Modified { earliest_cursor: 1 }
    );
}

#[test]
fn edit_transaction_failures_are_fully_atomic() {
    let mut project = project();
    let original = project.clone();
    let error = project
        .edit_transaction(|edit| {
            edit.fork_branch("main", 4, "temporary", "Temporary")?;
            anyhow::bail!("injected failure")
        })
        .unwrap_err();
    assert!(error.to_string().contains("injected failure"));
    assert_eq!(project, original);

    assert!(
        project
            .edit_transaction(|edit| edit.fork_branch("main", 13, "past-end", "Past End"))
            .is_err()
    );
    assert_eq!(project, original);

    assert!(
        project
            .edit_transaction(|edit| edit.fork_branch("main", 4, "alternate", "Duplicate"))
            .is_err()
    );
    assert_eq!(project, original);

    assert!(
        project
            .edit_transaction(|edit| edit.set_input_range("main", u64::MAX, 2, Default::default()))
            .is_err()
    );
    assert_eq!(project, original);

    assert!(
        project
            .edit_transaction(|edit| {
                edit.replace_branch_events(
                    "main",
                    vec![ReplayEvent::FdsDiskSide { frame: 13, side: 0 }],
                )
            })
            .is_err()
    );
    assert_eq!(project, original);

    project.edit_generation = u64::MAX;
    let before_overflow = project.clone();
    assert!(
        project
            .edit_transaction(|edit| {
                edit.set_input_range(
                    "main",
                    0,
                    1,
                    TasInputFrame {
                        players: [TasControllerInput {
                            buttons: 1,
                            dpad: 0,
                        }; 5],
                        ..TasInputFrame::default()
                    },
                )
            })
            .is_err()
    );
    assert_eq!(project, before_overflow);

    let mut rerecord_overflow = original.clone();
    rerecord_overflow.rerecord_count = u64::MAX;
    let before_rerecord_overflow = rerecord_overflow.clone();
    assert!(
        rerecord_overflow
            .edit_transaction(|edit| {
                edit.set_input_range(
                    "main",
                    0,
                    1,
                    TasInputFrame {
                        players: [TasControllerInput {
                            buttons: 2,
                            dpad: 0,
                        }; 5],
                        ..TasInputFrame::default()
                    },
                )
            })
            .is_err()
    );
    assert_eq!(rerecord_overflow, before_rerecord_overflow);
}

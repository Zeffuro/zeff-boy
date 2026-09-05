use std::collections::BTreeSet;

use anyhow::Result;
use zeff_emu_common::replay::ReplayEvent;

use super::super::edit::{TasProjectEdit, classify_branch_changes_for_test};
use super::*;

fn assert_matches_hashed_oracle(
    edit: impl FnOnce(&mut TasProjectEdit<'_>) -> Result<()>,
) -> (BTreeSet<String>, bool) {
    let before = project();
    let (structural, hashed) = classify_branch_changes_for_test(&before, edit).unwrap();
    assert_eq!(structural.changed_existing_ids, hashed.changed_existing_ids);
    assert_eq!(
        structural.divergent_fork_created,
        hashed.divergent_fork_created
    );
    (
        structural.changed_existing_ids,
        structural.divergent_fork_created,
    )
}

fn input(buttons: u8) -> TasInputFrame {
    TasInputFrame {
        players: [TasControllerInput { buttons, dpad: 1 }; 5],
        ..TasInputFrame::default()
    }
}

#[test]
fn structural_classification_matches_hashed_metadata_oracle() {
    assert_matches_hashed_oracle(|edit| {
        edit.set_project_comment("updated");
        edit.rename_branch("main", "Renamed")?;
        edit.set_branch_comment("alternate", "notes")
    });
}

#[test]
fn structural_classification_matches_hashed_movie_edit_oracle() {
    assert_matches_hashed_oracle(|edit| {
        edit.set_input_range("main", 1, 1, input(0x80))?;
        edit.replace_branch_events(
            "alternate",
            vec![ReplayEvent::FdsDiskSide { frame: 7, side: 0 }],
        )?;
        edit.insert_frames("main", 5, 1)
    });
}

#[test]
fn structural_classification_matches_hashed_source_then_equal_fork_oracle() {
    let (changed, divergent) = assert_matches_hashed_oracle(|edit| {
        edit.set_input_range("main", 5, 1, input(0x20))?;
        edit.fork_branch("main", 4, "route", "Route")
    });
    assert_eq!(changed, BTreeSet::from(["main".to_owned()]));
    assert!(!divergent);
}

#[test]
fn structural_classification_matches_hashed_fork_then_divergent_child_oracle() {
    let (changed, divergent) = assert_matches_hashed_oracle(|edit| {
        edit.fork_branch("main", 4, "route", "Route")?;
        edit.set_input_range("route", 5, 1, input(0x40))
    });
    assert!(changed.is_empty());
    assert!(divergent);
}

#[test]
fn structural_classification_matches_hashed_multiple_created_oracle() {
    let (changed, divergent) = assert_matches_hashed_oracle(|edit| {
        edit.fork_branch("main", 4, "route-a", "Route A")?;
        edit.fork_branch("main", 4, "route-b", "Route B")?;
        edit.set_input_range("route-b", 5, 1, input(0x10))
    });
    assert!(changed.is_empty());
    assert!(divergent);
}

#[test]
fn structural_classification_matches_hashed_deletion_oracle() {
    let (changed, divergent) = assert_matches_hashed_oracle(|edit| {
        edit.delete_branch_subtree("alternate")?;
        edit.set_input_range("main", 5, 1, input(0x08))
    });
    assert_eq!(changed, BTreeSet::from(["main".to_owned()]));
    assert!(!divergent);
}

#[test]
fn structural_classification_treats_reverted_edit_as_noop() {
    let (changed, divergent) = assert_matches_hashed_oracle(|edit| {
        edit.set_project_comment("changed");
        edit.set_project_comment("project notes");
        Ok(())
    });
    assert!(changed.is_empty());
    assert!(!divergent);
}

#[test]
fn metadata_preserves_verification_while_movie_edit_invalidates_it() {
    let mut tas = project();
    attach_current_verification(&mut tas, "main");
    let verification = tas.branch("main").unwrap().verification().cloned();
    tas.edit_transaction(|edit| edit.rename_branch("main", "Renamed"))
        .unwrap();
    assert_eq!(
        tas.branch("main").unwrap().verification(),
        verification.as_ref()
    );
    assert!(tas.verification_is_current("main").unwrap());
    tas.edit_transaction(|edit| edit.set_input_range("main", 5, 1, input(0x04)))
        .unwrap();
    assert!(tas.branch("main").unwrap().verification().is_none());
}

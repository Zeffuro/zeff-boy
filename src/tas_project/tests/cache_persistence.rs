use zeff_emu_common::replay::ReplayEvent;

use super::super::*;
use super::project;

#[test]
fn cache_prefix_changes_only_after_edited_input_is_consumed() {
    let project = project();
    let before = project.seek_cache_identity("main", 2).unwrap();
    let after = project.seek_cache_identity("main", 3).unwrap();
    let mut edited = project.clone();
    edited.branches[0].input_spans[0].input.players[0].buttons ^= 0x40;

    assert_eq!(edited.seek_cache_identity("main", 2).unwrap(), before);
    assert_ne!(edited.seek_cache_identity("main", 3).unwrap(), after);
}

#[test]
fn cache_prefix_changes_only_after_an_edited_event() {
    let project = project();
    let before = project.seek_cache_identity("main", 6).unwrap();
    let after = project.seek_cache_identity("main", 7).unwrap();
    let mut edited = project.clone();
    if let ReplayEvent::FdsDiskSide { side, .. } = &mut edited.branches[0].events[0] {
        *side = 0;
    }

    assert_eq!(edited.seek_cache_identity("main", 6).unwrap(), before);
    assert_ne!(edited.seek_cache_identity("main", 7).unwrap(), after);
}

#[test]
fn branch_snapshots_do_not_depend_on_live_parent_content() {
    let project = project();
    let alternate = project.branch_movie_sha256("alternate").unwrap();
    let mut edited = project.clone();
    edited.branches[0].input_spans[0].input.players[0].buttons ^= 0x20;

    assert_ne!(
        edited.branch_movie_sha256("main").unwrap(),
        project.branch_movie_sha256("main").unwrap()
    );
    assert_eq!(edited.branch_movie_sha256("alternate").unwrap(), alternate);
}

#[test]
fn verification_provenance_becomes_stale_after_a_movie_edit() {
    let mut project = project();
    let movie_hash = project.branch_movie_sha256("main").unwrap();
    project.branches[0].verification = Some(TasVerificationProvenance {
        branch_movie_sha256: movie_hash,
        checkpoints: vec![TasVerificationCheckpoint {
            cursor: 6,
            state_sha256: TasDigest([0x99; 32]),
        }],
        final_state_sha256: Some(TasDigest([0xAA; 32])),
    });
    assert!(project.verification_is_current("main").unwrap());
    assert_eq!(
        TasProject::decode(&project.encode().unwrap()).unwrap(),
        project
    );
    project.branches[0].input_spans[0].input.players[0].buttons ^= 0x20;
    assert!(!project.verification_is_current("main").unwrap());
}

#[test]
fn atomic_save_preserves_backup_and_recovers() {
    let directory =
        std::env::temp_dir().join(format!("zeff-tas-save-{}-{}", std::process::id(), "backup"));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("movie.ztas");
    let original = project();
    original.save_atomic(&path).unwrap();

    let mut updated = original.clone();
    updated.project_comment = "updated".to_owned();
    updated.edit_generation += 1;
    updated.save_atomic(&path).unwrap();
    assert_eq!(TasProject::load(&path).unwrap(), updated);
    assert_eq!(
        TasProject::load(&TasProject::backup_path(&path).unwrap()).unwrap(),
        original
    );

    std::fs::write(&path, b"corrupt").unwrap();
    let (recovered, source) = TasProject::load_with_backup(&path).unwrap();
    assert_eq!(source, TasProjectLoadSource::Backup);
    assert_eq!(recovered, original);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_atomic_save_keeps_the_previous_project() {
    let directory = std::env::temp_dir().join(format!(
        "zeff-tas-save-{}-{}",
        std::process::id(),
        "failure"
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("movie.ztas");
    let original = project();
    original.save_atomic(&path).unwrap();
    let mut invalid = original.clone();
    invalid.identity.start_state_sha256.0[0] ^= 1;

    assert!(invalid.save_atomic(&path).is_err());
    assert_eq!(TasProject::load(&path).unwrap(), original);
    std::fs::remove_dir_all(directory).unwrap();
}

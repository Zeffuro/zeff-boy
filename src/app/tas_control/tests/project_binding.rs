use super::*;
use crate::emu_backend::loader::{DirectGbTasExecutionLoader, DirectNesTasExecutionLoader};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasEditorSession, TasSeekStateCache,
};

fn acquired_with(
    coordinator: &mut TasControlCoordinator,
    request_id: u64,
    project: TasEditorControlSnapshot,
) -> ResponseDisposition {
    let current = project.clone();
    coordinator.consume_response(
        WORKER_GENERATION,
        acquired(request_id, 61),
        binding(project),
        Some(&current),
    )
}

#[test]
fn every_snapshot_field_participates_in_pending_acquire_correlation() {
    let baseline = snapshot(0, "main", 3);
    let variants = [
        TasEditorControlSnapshot {
            profile: crate::emu_thread::TasExecutionProfile::DirectGbRomOnlyDmg,
            ..baseline.clone()
        },
        TasEditorControlSnapshot {
            edit_generation: 1,
            ..baseline.clone()
        },
        TasEditorControlSnapshot {
            project_content_sha256: crate::tas_project::TasDigest([0x31; 32]),
            ..baseline.clone()
        },
        TasEditorControlSnapshot {
            sync_identity_sha256: crate::tas_project::TasDigest([0x32; 32]),
            ..baseline.clone()
        },
        TasEditorControlSnapshot {
            branch_id: "other".to_owned(),
            ..baseline.clone()
        },
        TasEditorControlSnapshot {
            cursor: 4,
            ..baseline.clone()
        },
        TasEditorControlSnapshot {
            execution_prefix_len: 5,
            ..baseline.clone()
        },
        TasEditorControlSnapshot {
            branch_prefix_sha256: crate::tas_project::TasDigest([0x33; 32]),
            ..baseline.clone()
        },
    ];

    for current in variants {
        let mut coordinator = TasControlCoordinator::new();
        let EmuCommand::AcquireTasControl { request_id, .. } = coordinator
            .begin_acquire(WORKER_GENERATION, baseline.clone())
            .unwrap()
        else {
            unreachable!()
        };

        bound_rollback(
            acquired_with(&mut coordinator, request_id, current),
            WORKER_GENERATION,
            61,
        );
        assert!(matches!(
            coordinator.state,
            TasControlState::RollbackPending { lease_id: 61, .. }
        ));
    }
}

#[test]
fn pending_editor_divergence_marks_acquire_for_exact_late_rollback() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    let changed = snapshot(1, "branch-b", 7);

    assert!(coordinator.reconcile_project(Some(&changed)).is_none());
    assert!(matches!(
        coordinator.state,
        TasControlState::AcquirePending {
            cancelled: true,
            project: TasEditorControlSnapshot {
                edit_generation: 0,
                ref branch_id,
                cursor: 0,
                ..
            },
            ..
        } if branch_id == "main"
    ));

    bound_rollback(
        acquired_with(&mut coordinator, request_id, changed),
        WORKER_GENERATION,
        61,
    );
}

#[test]
fn invalid_acquired_project_drops_payload_and_rolls_back_from_witness_proof() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);

    let response =
        coordinator.consume_response(WORKER_GENERATION, acquired(request_id, 62), None, None);

    bound_rollback(response, WORKER_GENERATION, 62);
    assert!(matches!(
        coordinator.state,
        TasControlState::RollbackPending {
            lease_id: 62,
            checkpoint_frame_count: 73,
            ..
        }
    ));
}

#[test]
fn executing_editor_selection_or_content_change_rolls_back_without_checkpoint_bytes() {
    let same_generation_divergence = TasEditorControlSnapshot {
        project_content_sha256: crate::tas_project::TasDigest([0x71; 32]),
        ..snapshot(0, "main", 0)
    };
    for changed in [
        snapshot(0, "other", 0),
        snapshot(8, "main", 0),
        same_generation_divergence,
    ] {
        let mut coordinator = TasControlCoordinator::new();
        let request_id = acquire(&mut coordinator);
        consume(&mut coordinator, acquired(request_id, 41));

        let rollback = coordinator.reconcile_project(Some(&changed)).unwrap();

        assert!(matches!(
            rollback.into_parts_for_worker(WORKER_GENERATION),
            Some((
                WORKER_GENERATION,
                EmuCommand::RollbackTasControl { lease_id: 41 }
            ))
        ));
        assert!(matches!(
            coordinator.state,
            TasControlState::RollbackPending {
                worker_generation: WORKER_GENERATION,
                lease_id: 41,
                ..
            }
        ));
    }
}

#[test]
fn direct_gb_project_binding_accepts_exact_witness_and_rejects_mismatch() {
    let directory = crate::test_support::test_directory("tas-control-gb-binding").unwrap();
    let source_path = directory.path().join("game.gb");
    std::fs::write(&source_path, crate::test_support::build_gb_test_rom()).unwrap();
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project().unwrap();
    let manual_path = directory.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let snapshot = TasEditorControlSnapshot::capture(&session).unwrap();
    let identity = session.project().identity();
    let state = session.project().start_state().to_vec();
    let witness = TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectGbRomOnlyDmg,
        frame_count: 0,
        source_media_sha256: identity.source_media_sha256,
        effective_media_sha256: identity.effective_media_sha256,
        current_state_sha256: crate::tas_project::TasDigest::from_bytes(&state),
        current_state_bytes: state,
        determinism_abi: zeff_gb_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id: zeff_gb_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: identity.sync_config_sha256,
    };
    assert!(TasEditorControlSnapshot::validate_acquired(&session, &witness).is_ok());

    let mut mismatched = witness.clone();
    mismatched.sync_config_sha256 = crate::tas_project::TasDigest([0x7E; 32]);
    assert!(TasEditorControlSnapshot::validate_acquired(&session, &mismatched).is_err());
    assert_eq!(snapshot.profile, TasExecutionProfile::DirectGbRomOnlyDmg);
}

#[test]
fn direct_nes_end_cursor_binds_the_complete_movie_for_append_recording() {
    let directory = crate::test_support::test_directory("tas-control-nes-end-cursor").unwrap();
    let source_path = directory.path().join("game.nes");
    std::fs::write(&source_path, crate::test_support::build_nes_test_rom()).unwrap();
    let project = DirectNesTasExecutionLoader::new(source_path, Vec::new())
        .create_project()
        .unwrap();
    let manual_path = directory.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache")).unwrap();
    let mut session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let frame_count = session.selected_branch().frame_count();
    session.set_cursor(frame_count).unwrap();
    let identity = session.project().identity();
    let state = session.project().start_state().to_vec();
    let witness = TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectNesCartridge,
        frame_count: 0,
        source_media_sha256: identity.source_media_sha256,
        effective_media_sha256: identity.effective_media_sha256,
        current_state_sha256: crate::tas_project::TasDigest::from_bytes(&state),
        current_state_bytes: state,
        determinism_abi: zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id: zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: identity.sync_config_sha256,
    };

    let binding = TasEditorControlSnapshot::validate_acquired(&session, &witness).unwrap();
    assert_eq!(binding.total_input_frames, frame_count);
    assert_eq!(binding.input_prefix.len() as u64, frame_count);
    assert_eq!(binding.snapshot.cursor, frame_count);
    assert_eq!(binding.snapshot.execution_prefix_len, frame_count);
}

use super::*;
use crate::emu_backend::loader::{
    DirectColecoTasExecutionLoader, DirectGbTasExecutionLoader, DirectNesTasExecutionLoader,
};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasColecoControllerInput, TasColecoKeypadKey,
    TasEditorSession, TasInputFrame, TasSeekStateCache,
};

static TEST_COLECO_BIOS: [u8; zeff_coleco_core::constants::BIOS_SIZE] =
    [0; zeff_coleco_core::constants::BIOS_SIZE];

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
            profile: crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg,
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
            branch_frame_count: 4,
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
fn linked_selection_and_equivalent_prefix_changes_keep_the_lease() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 41));
    assert!(
        coordinator
            .reconcile_project(Some(&snapshot(0, "main", 1)))
            .is_none()
    );

    let equivalent_branch = TasEditorControlSnapshot {
        project_content_sha256: crate::tas_project::TasDigest([0x71; 32]),
        ..snapshot(8, "route-b", 0)
    };
    assert!(
        coordinator
            .reconcile_project(Some(&equivalent_branch))
            .is_none()
    );
    assert!(matches!(
        coordinator.state,
        TasControlState::ExecutionPending {
            project: TasEditorControlSnapshot {
                edit_generation: 8,
                ref branch_id,
                ..
            },
            ..
        } if branch_id == "route-b"
    ));

    let longer_suffix = TasEditorControlSnapshot {
        edit_generation: 9,
        project_content_sha256: crate::tas_project::TasDigest([0x74; 32]),
        branch_frame_count: 2,
        ..equivalent_branch
    };
    assert!(
        coordinator
            .reconcile_project(Some(&longer_suffix))
            .is_none()
    );
    assert!(matches!(
        coordinator.state,
        TasControlState::ExecutionPending {
            project: TasEditorControlSnapshot {
                edit_generation: 9,
                branch_frame_count: 2,
                ..
            },
            ..
        }
    ));

    let wrong_sync = TasEditorControlSnapshot {
        sync_identity_sha256: crate::tas_project::TasDigest([0x72; 32]),
        ..snapshot(8, "main", 0)
    };
    let wrong_prefix = TasEditorControlSnapshot {
        branch_prefix_sha256: crate::tas_project::TasDigest([0x73; 32]),
        ..snapshot(8, "main", 0)
    };
    for changed in [snapshot(0, "other", 1), wrong_sync, wrong_prefix] {
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
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
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
    assert_eq!(snapshot.profile, TasExecutionProfile::DirectGbCartridgeDmg);
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
    let start_snapshot = TasEditorControlSnapshot::capture(&session).unwrap();
    assert_eq!(start_snapshot.cursor, 0);
    assert_eq!(start_snapshot.execution_prefix_len, 0);
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

#[test]
fn linked_boundary_capture_does_not_move_the_editor_selection() {
    let directory = crate::test_support::test_directory("tas-control-linked-boundary").unwrap();
    let source_path = directory.path().join("game.nes");
    std::fs::write(&source_path, crate::test_support::build_nes_test_rom()).unwrap();
    let mut project = DirectNesTasExecutionLoader::new(source_path, Vec::new())
        .create_project()
        .unwrap();
    project
        .edit_transaction(|edit| edit.insert_frames("main", 0, 5))
        .unwrap();
    let manual_path = directory.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache")).unwrap();
    let mut session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    session.set_cursor(5).unwrap();

    let linked = TasEditorControlSnapshot::capture_at(&session, 2).unwrap();

    assert_eq!(session.cursor(), 5);
    assert_eq!(linked.cursor, 2);
    assert_eq!(linked.execution_prefix_len, 2);
    assert_eq!(
        linked.branch_prefix_sha256,
        session.project().branch_prefix_sha256("main", 2).unwrap()
    );
}

#[test]
fn linked_binding_materializes_both_semantic_coleco_controllers() {
    let directory = crate::test_support::test_directory("tas-control-linked-coleco").unwrap();
    let source_path = directory.path().join("game.col");
    let mut source = vec![0; 8 * 1024];
    source[..2].copy_from_slice(&[0xAA, 0x55]);
    std::fs::write(&source_path, source).unwrap();
    let loader = DirectColecoTasExecutionLoader::new_with_bios_override(
        source_path,
        Vec::new(),
        &TEST_COLECO_BIOS,
    );
    let mut project = loader.create_project().unwrap();
    let controllers = [
        TasColecoControllerInput {
            up: true,
            keypad: TasColecoKeypadKey::Star,
            ..TasColecoControllerInput::default()
        },
        TasColecoControllerInput {
            right_button: true,
            keypad: TasColecoKeypadKey::Nine,
            ..TasColecoControllerInput::default()
        },
    ];
    project
        .edit_transaction(|edit| {
            edit.set_input_range(
                "main",
                0,
                1,
                TasInputFrame {
                    coleco: controllers,
                    ..TasInputFrame::default()
                },
            )
        })
        .unwrap();
    let manual_path = directory.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let profile = crate::emu_thread::TasExecutionProfile::DirectColecoCartridge;
    let binding = TasEditorControlSnapshot::prepare_linked_seek_at(
        &session,
        1,
        profile,
        session.project().sync_identity_sha256().unwrap(),
        &[],
    )
    .unwrap();

    assert_eq!(binding.snapshot.profile, profile);
    assert_eq!(binding.input_prefix[0].coleco, controllers);
}

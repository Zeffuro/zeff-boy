use super::*;

fn begin_execution(
    coordinator: &mut TasControlCoordinator,
    lease_id: u64,
) -> (u64, WorkerBoundCommand) {
    let request_id = acquire(coordinator);
    let ResponseDisposition::Consumed {
        follow_up: Some(command),
    } = consume(coordinator, acquired(request_id, lease_id))
    else {
        panic!("acquisition should enqueue execution");
    };
    let run_id = match &coordinator.state {
        TasControlState::ExecutionPending { run_id, .. } => *run_id,
        _ => panic!("acquisition should become execution pending"),
    };
    (run_id, command)
}

#[test]
fn acquisition_enqueues_exact_generation_lease_and_run_bound_payload() {
    let mut coordinator = TasControlCoordinator::new();
    let (run_id, command) = begin_execution(&mut coordinator, 41);

    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::ExecuteTasControl(request)
        )) if request.lease_id == 41
            && request.run_id == run_id
            && request.cache_proof.sync_identity_sha256 == crate::tas_project::TasDigest([0x22; 32])
            && request.cache_proof.branch_prefix_sha256 == crate::tas_project::TasDigest([0x23; 32])
            && request.cache_proof.target_cursor == 0
            && request.start_state_bytes == [0xAA]
            && request.input_prefix == [crate::emu_thread::TasInputFrame::default()]
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::ExecutionPending {
            worker_generation: WORKER_GENERATION,
            lease_id: 41,
            run_id: 1,
            ..
        }
    ));
}

#[test]
fn zero_boundary_execution_links_at_project_start() {
    let mut coordinator = TasControlCoordinator::new();
    let project = snapshot(0, "main", 0);
    let request_id = match coordinator
        .begin_acquire(WORKER_GENERATION, project.clone())
        .unwrap()
    {
        EmuCommand::AcquireTasControl { request_id, .. } => request_id,
        _ => unreachable!(),
    };
    let response = coordinator.consume_response(
        WORKER_GENERATION,
        acquired(request_id, 53),
        Some(TasAcquiredProjectBinding {
            snapshot: project.clone(),
            intermediate_cache_proofs: Vec::new(),
            predecessor_window: None,
            start_state_bytes: vec![0xAA],
            input_prefix: Vec::new(),
            total_input_frames: 0,
        }),
        Some(&project),
    );
    let ResponseDisposition::Consumed {
        follow_up: Some(command),
    } = response
    else {
        panic!("acquisition should enqueue zero-boundary execution");
    };
    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((WORKER_GENERATION, EmuCommand::ExecuteTasControl(request)))
            if request.lease_id == 53
                && request.run_id == 1
                && request.start_state_bytes == [0xAA]
                && request.input_prefix.is_empty()
    ));

    let response = coordinator.consume_response(
        WORKER_GENERATION,
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectNesCartridge,
            lease_id: 53,
            run_id: 1,
            segment_id: 1,
            segment_frame_count: 0,
            executed_project_frames: 0,
            frame_count: 0,
            state_sha256: crate::tas_project::TasDigest([0x30; 32]),
        },
        None,
        Some(&project),
    );
    assert!(matches!(
        response,
        ResponseDisposition::Consumed { follow_up: None }
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::AwaitingDecision {
            lease_id: 53,
            run_id: 1,
            candidate_segment_id: 1,
            candidate_segment_frame_count: 0,
            candidate_executed_project_frames: 0,
            candidate_frame_count: 0,
            ..
        }
    ));
    assert_eq!(coordinator.linked_cursor(), Some(0));
    assert!(coordinator.take_framebuffer_refresh());
}

#[test]
fn matching_completion_awaits_explicit_commit_or_rollback() {
    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 42);

    complete_execution(&mut coordinator, 42, run_id);

    assert!(coordinator.take_framebuffer_refresh());
    assert!(!coordinator.take_framebuffer_refresh());

    assert!(matches!(
        coordinator.state,
        TasControlState::AwaitingDecision {
            worker_generation: WORKER_GENERATION,
            lease_id: 42,
            run_id: 1,
            candidate_frame_count: 19,
            candidate_state_sha256,
            ..
        } if candidate_state_sha256 == crate::tas_project::TasDigest([0x31; 32])
    ));
    let current = snapshot(0, "main", 0);
    assert!(matches!(
        coordinator
            .commit(Some(&current))
            .unwrap()
            .into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::CommitTasControl { lease_id: 42 }
        ))
    ));
}

#[test]
fn linked_seek_reuses_the_lease_with_the_next_run_and_new_snapshot() {
    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 52);
    complete_execution(&mut coordinator, 52, run_id);
    let project = snapshot(1, "main", 2);
    let command = coordinator
        .begin_linked_seek(TasAcquiredProjectBinding {
            snapshot: project.clone(),
            intermediate_cache_proofs: Vec::new(),
            predecessor_window: None,
            start_state_bytes: vec![0xBB],
            input_prefix: vec![crate::emu_thread::TasInputFrame::default(); 3],
            total_input_frames: 3,
        })
        .unwrap();

    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((WORKER_GENERATION, EmuCommand::ExecuteTasControl(request)))
            if request.lease_id == 52
                && request.run_id == 2
                && request.start_state_bytes == [0xBB]
                && request.input_prefix.len() == 3
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::ExecutionPending {
            lease_id: 52,
            run_id: 2,
            project: ref actual,
            total_input_frames: 3,
            ..
        } if *actual == project
    ));
    assert!(coordinator.reconcile_project(Some(&project)).is_none());
}

#[test]
fn linked_fixed_length_edit_reconstructs_once_to_the_range_end() {
    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 55);
    complete_execution(&mut coordinator, 55, run_id);
    let project = TasEditorControlSnapshot {
        edit_generation: 1,
        project_content_sha256: crate::tas_project::TasDigest([0x71; 32]),
        cursor: 1,
        execution_prefix_len: 1,
        branch_prefix_sha256: crate::tas_project::TasDigest([0x72; 32]),
        ..snapshot(0, "main", 0)
    };

    let command = coordinator
        .begin_linked_edit_follow(
            TasAcquiredProjectBinding {
                snapshot: project.clone(),
                intermediate_cache_proofs: Vec::new(),
                predecessor_window: None,
                start_state_bytes: vec![0xBB],
                input_prefix: vec![crate::emu_thread::TasInputFrame::default()],
                total_input_frames: 1,
            },
            0,
            1,
        )
        .unwrap();

    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((WORKER_GENERATION, EmuCommand::ExecuteTasControl(request)))
            if request.lease_id == 55
                && request.run_id == 2
                && request.cache_proof.target_cursor == 1
                && request.cache_proof.branch_prefix_sha256
                    == crate::tas_project::TasDigest([0x72; 32])
                && request.input_prefix.len() == 1
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::ExecutionPending {
            lease_id: 55,
            run_id: 2,
            project: ref actual,
            total_input_frames: 1,
            ..
        } if *actual == project
    ));
    assert_eq!(coordinator.project_binding_cursor(), Some(1));
    assert!(
        coordinator
            .begin_linked_edit_follow(
                TasAcquiredProjectBinding {
                    snapshot: project,
                    intermediate_cache_proofs: Vec::new(),
                    predecessor_window: None,
                    start_state_bytes: vec![0xBB],
                    input_prefix: vec![crate::emu_thread::TasInputFrame::default()],
                    total_input_frames: 1,
                },
                0,
                1,
            )
            .is_err()
    );
}

#[test]
fn linked_edit_follow_rejects_structural_or_uncommitted_changes() {
    for project in [
        TasEditorControlSnapshot {
            edit_generation: 1,
            project_content_sha256: crate::tas_project::TasDigest([0x71; 32]),
            branch_frame_count: 2,
            cursor: 1,
            execution_prefix_len: 1,
            ..snapshot(0, "main", 0)
        },
        TasEditorControlSnapshot {
            edit_generation: 2,
            project_content_sha256: crate::tas_project::TasDigest([0x71; 32]),
            cursor: 1,
            execution_prefix_len: 1,
            ..snapshot(0, "main", 0)
        },
        TasEditorControlSnapshot {
            edit_generation: 1,
            cursor: 1,
            execution_prefix_len: 1,
            ..snapshot(0, "main", 0)
        },
    ] {
        let mut coordinator = TasControlCoordinator::new();
        let (run_id, _) = begin_execution(&mut coordinator, 56);
        complete_execution(&mut coordinator, 56, run_id);
        let result = coordinator.begin_linked_edit_follow(
            TasAcquiredProjectBinding {
                snapshot: project,
                intermediate_cache_proofs: Vec::new(),
                predecessor_window: None,
                start_state_bytes: vec![0xBB],
                input_prefix: vec![crate::emu_thread::TasInputFrame::default()],
                total_input_frames: 1,
            },
            0,
            1,
        );

        assert!(result.is_err());
        assert!(matches!(
            coordinator.state,
            TasControlState::AwaitingDecision { lease_id: 56, .. }
        ));
    }
}

#[test]
fn exact_worker_cache_hit_can_complete_a_longer_linked_seek_at_its_target() {
    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 54);
    complete_execution(&mut coordinator, 54, run_id);
    let project = snapshot(1, "main", 2);
    coordinator
        .begin_linked_seek(TasAcquiredProjectBinding {
            snapshot: project.clone(),
            intermediate_cache_proofs: Vec::new(),
            predecessor_window: None,
            start_state_bytes: vec![0xBB],
            input_prefix: vec![crate::emu_thread::TasInputFrame::default(); 3],
            total_input_frames: 3,
        })
        .unwrap();

    let response = coordinator.consume_response(
        WORKER_GENERATION,
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectNesCartridge,
            lease_id: 54,
            run_id: 2,
            segment_id: 1,
            segment_frame_count: 0,
            executed_project_frames: 3,
            frame_count: 21,
            state_sha256: crate::tas_project::TasDigest([0x61; 32]),
        },
        None,
        Some(&project),
    );

    assert!(matches!(
        response,
        ResponseDisposition::Consumed { follow_up: None }
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::AwaitingDecision {
            lease_id: 54,
            run_id: 2,
            candidate_segment_frame_count: 0,
            candidate_executed_project_frames: 3,
            ..
        }
    ));
}

#[test]
fn typed_execution_failure_rolls_back_but_authority_ambiguity_terminalizes() {
    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 43);
    let response = consume(
        &mut coordinator,
        EmuResponse::TasExecutionRejected {
            profile: TasExecutionProfile::DirectNesCartridge,
            requested_lease_id: 43,
            run_id,
            reason: TasExecutionRejectedReason::FrameProgressFailed,
        },
    );
    bound_rollback(response, WORKER_GENERATION, 43);

    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 44);
    consume(
        &mut coordinator,
        EmuResponse::TasExecutionRejected {
            profile: TasExecutionProfile::DirectNesCartridge,
            requested_lease_id: 44,
            run_id,
            reason: TasExecutionRejectedReason::NoActiveLease,
        },
    );
    assert!(matches!(
        coordinator.state,
        TasControlState::Terminal {
            reason: TasControlTerminalReason::ExecutionAuthorityMismatch,
            ..
        }
    ));
}

#[test]
fn execution_response_with_a_wrong_profile_terminalizes_before_refresh() {
    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 51);
    let current = snapshot(0, "main", 0);

    let response = coordinator.consume_response(
        WORKER_GENERATION,
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectGbCartridgeDmg,
            lease_id: 51,
            run_id,
            segment_id: 1,
            segment_frame_count: 1,
            executed_project_frames: 1,
            frame_count: 1,
            state_sha256: crate::tas_project::TasDigest([5; 32]),
        },
        None,
        Some(&current),
    );

    assert!(matches!(
        response,
        ResponseDisposition::Consumed { follow_up: None }
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::Terminal {
            reason: TasControlTerminalReason::ExecutionResponseMismatch,
            ..
        }
    ));
    assert!(!coordinator.take_framebuffer_refresh());
}

#[test]
fn same_generation_wrong_run_or_project_divergence_never_awaits_decision() {
    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 45);
    complete_execution(&mut coordinator, 45, run_id + 1);
    assert!(matches!(
        coordinator.state,
        TasControlState::Terminal {
            reason: TasControlTerminalReason::ExecutionResponseMismatch,
            ..
        }
    ));

    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 46);
    let changed = snapshot(1, "main", 0);
    let response = coordinator.consume_response(
        WORKER_GENERATION,
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectNesCartridge,
            lease_id: 46,
            run_id,
            segment_id: 1,
            segment_frame_count: 1,
            executed_project_frames: 1,
            frame_count: 20,
            state_sha256: crate::tas_project::TasDigest([0x41; 32]),
        },
        None,
        Some(&changed),
    );
    bound_rollback(response, WORKER_GENERATION, 46);
}

#[test]
fn commit_with_changed_project_becomes_rollback() {
    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 47);
    complete_execution(&mut coordinator, 47, run_id);
    let changed = snapshot(0, "other", 0);

    let command = coordinator.commit(Some(&changed)).unwrap();

    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::RollbackTasControl { lease_id: 47 }
        ))
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::RollbackPending { lease_id: 47, .. }
    ));
}

#[test]
fn a_new_lease_resets_its_run_identifier_sequence() {
    let mut coordinator = TasControlCoordinator::new();
    let (first_run, _) = begin_execution(&mut coordinator, 48);
    complete_execution(&mut coordinator, 48, first_run);
    let current = snapshot(0, "main", 0);
    let commit = coordinator.commit(Some(&current)).unwrap();
    assert!(matches!(
        commit.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::CommitTasControl { lease_id: 48 }
        ))
    ));
    consume(
        &mut coordinator,
        EmuResponse::TasControlCommitted { lease_id: 48 },
    );
    let (second_run, _) = begin_execution(&mut coordinator, 49);

    assert_eq!(first_run, 1);
    assert_eq!(second_run, 1);
}

#[test]
fn live_status_tracks_execution_and_uses_plain_terminal_text() {
    let mut coordinator = TasControlCoordinator::new();
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Ready {
            recording_available: false,
        }
    );
    let request_id = acquire(&mut coordinator);
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Acquiring
    );
    consume(&mut coordinator, acquired(request_id, 49));
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Staging {
            completed: 0,
            total: 1,
        }
    );
    coordinator.terminalize_worker(
        WORKER_GENERATION,
        TasControlTerminalReason::ResponseChannelClosed,
    );
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Terminal(
            "The emulator worker became unavailable".to_owned()
        )
    );
}

#[test]
fn late_execution_response_from_another_worker_generation_is_inert() {
    let mut coordinator = TasControlCoordinator::new();
    let (run_id, _) = begin_execution(&mut coordinator, 50);
    let current = snapshot(0, "main", 0);

    let response = coordinator.consume_response(
        WORKER_GENERATION + 1,
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectNesCartridge,
            lease_id: 50,
            run_id,
            segment_id: 1,
            segment_frame_count: 1,
            executed_project_frames: 1,
            frame_count: 1,
            state_sha256: crate::tas_project::TasDigest([5; 32]),
        },
        None,
        Some(&current),
    );

    assert!(matches!(
        response,
        ResponseDisposition::Consumed { follow_up: None }
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::ExecutionPending {
            worker_generation: WORKER_GENERATION,
            lease_id: 50,
            ..
        }
    ));
    assert!(!coordinator.take_framebuffer_refresh());
}

#[test]
fn exact_rollback_success_refreshes_once_but_mismatch_does_not() {
    let mut coordinator = TasControlCoordinator::new();
    let (_run_id, _) = begin_execution(&mut coordinator, 51);
    coordinator.cancel().unwrap();

    consume(&mut coordinator, rolled_back(51, 73));

    assert_eq!(coordinator.state, TasControlState::Detached);
    assert!(coordinator.take_framebuffer_refresh());
    assert!(!coordinator.take_framebuffer_refresh());

    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 52));
    coordinator.cancel().unwrap();
    consume(
        &mut coordinator,
        EmuResponse::TasControlRolledBack {
            lease_id: 52,
            restored_state_sha256: crate::tas_project::TasDigest([8; 32]),
            frame_count: 73,
        },
    );
    assert!(!coordinator.take_framebuffer_refresh());
}

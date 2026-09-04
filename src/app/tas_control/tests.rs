use super::*;
use crate::emu_thread::{
    TasControlAcquireRejectedReason, TasControlCommandKind, TasControlCommitRejectedReason,
    TasControlLeaseWitness, TasControlRollbackRejectedReason, TasExecutionProfile,
};

mod coleco_recording_roundtrip;
mod execution;
mod fds_recording_roundtrip;
mod game_gear_recording_roundtrip;
mod gba_recording_roundtrip;
mod gbc_recording_roundtrip;
mod harness;
mod live_recording;
mod pause_ownership;
mod pce_archive_recording_roundtrip;
mod pce_recording_roundtrip;
mod playback;
mod playback_roundtrip;
mod project_binding;
mod repair_roundtrip;
mod sg1000_recording_roundtrip;
mod sms_recording_roundtrip;
mod unavailability;
mod ws_recording_roundtrip;

const WORKER_GENERATION: u64 = 4;

fn acquire(coordinator: &mut TasControlCoordinator) -> u64 {
    match coordinator
        .begin_acquire(WORKER_GENERATION, snapshot(0, "main", 0))
        .unwrap()
    {
        EmuCommand::AcquireTasControl {
            request_id,
            profile: TasExecutionProfile::DirectNesCartridge,
        } => request_id,
        _ => unreachable!(),
    }
}

fn snapshot(edit_generation: u64, branch_id: &str, cursor: u64) -> TasEditorControlSnapshot {
    TasEditorControlSnapshot {
        profile: TasExecutionProfile::DirectNesCartridge,
        edit_generation,
        project_content_sha256: crate::tas_project::TasDigest([0x21; 32]),
        sync_identity_sha256: crate::tas_project::TasDigest([0x22; 32]),
        branch_id: branch_id.to_owned(),
        branch_frame_count: cursor.max(1),
        cursor,
        execution_prefix_len: cursor,
        branch_prefix_sha256: crate::tas_project::TasDigest([0x23; 32]),
    }
}

fn binding(value: TasEditorControlSnapshot) -> Option<TasAcquiredProjectBinding> {
    Some(TasAcquiredProjectBinding {
        snapshot: value,
        intermediate_cache_proofs: Vec::new(),
        predecessor_window: None,
        start_state_bytes: vec![0xAA],
        input_prefix: vec![crate::emu_thread::TasInputFrame::default()],
        total_input_frames: 1,
    })
}

fn acquired(request_id: u64, lease_id: u64) -> EmuResponse {
    EmuResponse::TasControlAcquired {
        request_id,
        lease_id,
        witness: witness(73),
    }
}

fn witness(frame_count: u64) -> Box<TasControlLeaseWitness> {
    Box::new(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectNesCartridge,
        frame_count,
        source_media_sha256: crate::tas_project::TasDigest([1; 32]),
        effective_media_sha256: crate::tas_project::TasDigest([1; 32]),
        current_state_bytes: vec![3, 4],
        current_state_sha256: crate::tas_project::TasDigest::from_bytes(&[3, 4]),
        determinism_abi: zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id: zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: crate::emu_backend::loader::direct_nes_tas_sync_config_sha256(),
    })
}

fn consume(coordinator: &mut TasControlCoordinator, response: EmuResponse) -> ResponseDisposition {
    let acquired_project = matches!(&response, EmuResponse::TasControlAcquired { .. }).then(|| {
        TasAcquiredProjectBinding {
            snapshot: snapshot(0, "main", 0),
            intermediate_cache_proofs: Vec::new(),
            predecessor_window: None,
            start_state_bytes: vec![0xAA],
            input_prefix: vec![crate::emu_thread::TasInputFrame::default()],
            total_input_frames: 1,
        }
    });
    let current = snapshot(0, "main", 0);
    coordinator.consume_response(
        WORKER_GENERATION,
        response,
        acquired_project,
        Some(&current),
    )
}

fn complete_execution(coordinator: &mut TasControlCoordinator, lease_id: u64, run_id: u64) {
    consume(
        coordinator,
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectNesCartridge,
            lease_id,
            run_id,
            segment_id: 1,
            segment_frame_count: 1,
            executed_project_frames: 1,
            frame_count: 19,
            state_sha256: crate::tas_project::TasDigest([0x31; 32]),
        },
    );
}

#[test]
fn recording_acquire_requests_realtime_only_after_staging_completes() {
    let mut coordinator = TasControlCoordinator::new();
    let project = snapshot(0, "main", 0);
    coordinator
        .queue_acquire(WORKER_GENERATION, project, TasControlStartMode::Record)
        .unwrap();
    let EmuCommand::AcquireTasControl { request_id, .. } = coordinator
        .begin_queued_acquire()
        .unwrap()
        .expect("the queued recording acquisition should start")
    else {
        unreachable!();
    };

    assert!(!coordinator.take_realtime_recording_start_request());
    consume(&mut coordinator, acquired(request_id, 60));
    assert!(!coordinator.take_realtime_recording_start_request());
    complete_execution(&mut coordinator, 60, 1);
    assert!(coordinator.take_realtime_recording_start_request());
    assert!(!coordinator.take_realtime_recording_start_request());
}

#[test]
fn cancelling_recording_acquire_clears_realtime_start_request() {
    let mut coordinator = TasControlCoordinator::new();
    coordinator
        .queue_acquire(
            WORKER_GENERATION,
            snapshot(0, "main", 0),
            TasControlStartMode::Record,
        )
        .unwrap();
    assert!(coordinator.cancel().is_none());
    assert!(!coordinator.take_realtime_recording_start_request());
}

#[test]
fn recording_acquire_queues_game_boy_without_changing_authority() {
    let mut coordinator = TasControlCoordinator::new();
    let project = TasEditorControlSnapshot {
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
        ..snapshot(0, "main", 0)
    };

    coordinator
        .queue_acquire(WORKER_GENERATION, project, TasControlStartMode::Record)
        .unwrap();
    let EmuCommand::AcquireTasControl { profile, .. } = coordinator
        .begin_queued_acquire()
        .unwrap()
        .expect("the queued Game Boy recording acquisition should start")
    else {
        unreachable!();
    };
    assert_eq!(profile, TasExecutionProfile::DirectGbCartridgeDmg);
    assert!(!coordinator.take_realtime_recording_start_request());
}

fn bound_rollback(response: ResponseDisposition, generation: u64, lease_id: u64) {
    let ResponseDisposition::Consumed {
        follow_up: Some(bound),
    } = response
    else {
        panic!("expected generation-bound rollback");
    };
    assert_eq!(bound.worker_generation, generation);
    assert!(matches!(
        bound.into_parts_for_worker(generation),
        Some((actual_generation, EmuCommand::RollbackTasControl {
            lease_id: actual_lease_id,
        })) if actual_generation == generation && actual_lease_id == lease_id
    ));
}

fn rolled_back(lease_id: u64, frame_count: u64) -> EmuResponse {
    EmuResponse::TasControlRolledBack {
        lease_id,
        restored_state_sha256: crate::tas_project::TasDigest::from_bytes(&[3, 4]),
        frame_count,
    }
}

#[test]
fn acquire_success_starts_exact_lease_bound_run_without_checkpoint_bytes() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);

    let response = consume(&mut coordinator, acquired(request_id, 41));

    assert!(matches!(
        response,
        ResponseDisposition::Consumed {
            follow_up: Some(_),
            ..
        }
    ));
    assert_eq!(
        coordinator.state,
        TasControlState::ExecutionPending {
            worker_generation: WORKER_GENERATION,
            lease_id: 41,
            run_id: 1,
            proof: TasControlHeldProof::from_witness(&witness(73)),
            project: snapshot(0, "main", 0),
            total_input_frames: 1,
            predecessor_source_cursors: Vec::new(),
        }
    );
    assert!(!coordinator.gameplay_commands_allowed());
}

#[test]
fn matching_acquire_rejection_detaches() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);

    let response = consume(
        &mut coordinator,
        EmuResponse::TasControlAcquireRejected {
            request_id,
            reason: TasControlAcquireRejectedReason::PendingFrameDelivery,
        },
    );

    assert!(matches!(response, ResponseDisposition::Consumed { .. }));
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn stale_acquired_response_enters_tracked_rollback() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);

    let response = consume(&mut coordinator, acquired(request_id + 1, 99));

    bound_rollback(response, WORKER_GENERATION, 99);
    assert!(matches!(
        coordinator.state,
        TasControlState::RollbackPending {
            lease_id: 99,
            checkpoint_frame_count: 73,
            ..
        }
    ));

    consume(
        &mut coordinator,
        EmuResponse::TasControlRollbackRejected {
            requested_lease_id: 99,
            reason: TasControlRollbackRejectedReason::NoActiveLease,
        },
    );
    assert!(matches!(
        coordinator.state,
        TasControlState::Terminal {
            reason: TasControlTerminalReason::RollbackRejected,
            ..
        }
    ));
}

#[test]
fn response_from_another_worker_generation_is_stale() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);

    let response = coordinator.consume_response(
        WORKER_GENERATION + 1,
        acquired(request_id, 99),
        binding(snapshot(0, "main", 0)),
        Some(&snapshot(0, "main", 0)),
    );

    let ResponseDisposition::Consumed {
        follow_up: Some(bound),
    } = response
    else {
        panic!("expected generation-bound rollback");
    };
    assert_eq!(bound.worker_generation, WORKER_GENERATION + 1);
    assert!(bound.into_parts_for_worker(WORKER_GENERATION).is_none());
    assert!(matches!(
        coordinator.state,
        TasControlState::AcquirePending {
            worker_generation: WORKER_GENERATION,
            request_id: pending,
            ..
        } if pending == request_id
    ));
}

#[test]
fn acquisition_waits_for_all_in_flight_delivery() {
    assert!(acquisition_delivery_quiesced(0));
    assert!(!acquisition_delivery_quiesced(1));
    assert!(!acquisition_delivery_quiesced(2));
}

#[test]
fn queued_acquisition_fences_gameplay_until_frame_delivery_is_quiescent() {
    let mut coordinator = TasControlCoordinator::new();
    let project = snapshot(0, "main", 0);

    coordinator
        .queue_acquire(
            WORKER_GENERATION,
            project.clone(),
            TasControlStartMode::Record,
        )
        .unwrap();
    assert!(!coordinator.gameplay_commands_allowed());
    assert!(matches!(
        coordinator.state,
        TasControlState::AcquireQueued {
            worker_generation: WORKER_GENERATION,
            project: ref queued,
        } if *queued == project
    ));

    let EmuCommand::AcquireTasControl {
        request_id,
        profile,
    } = coordinator
        .begin_queued_acquire()
        .unwrap()
        .expect("the queued acquisition should begin once delivery has drained")
    else {
        unreachable!();
    };
    assert_eq!(request_id, 1);
    assert_eq!(profile, TasExecutionProfile::DirectNesCartridge);
    assert!(matches!(
        coordinator.state,
        TasControlState::AcquirePending {
            request_id: 1,
            cancelled: false,
            ..
        }
    ));
}

#[test]
fn cancelling_a_queued_acquisition_restores_normal_gameplay() {
    let mut coordinator = TasControlCoordinator::new();
    coordinator
        .queue_acquire(
            WORKER_GENERATION,
            snapshot(0, "main", 0),
            TasControlStartMode::Preview,
        )
        .unwrap();

    assert!(coordinator.cancel().is_none());
    assert_eq!(coordinator.state, TasControlState::Detached);
    assert!(coordinator.gameplay_commands_allowed());
    assert!(!coordinator.take_realtime_recording_start_request());
}

#[test]
fn matching_rollback_is_the_only_rollback_that_detaches() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 41));
    let rollback = coordinator.cancel().unwrap();
    assert!(matches!(
        rollback.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::RollbackTasControl { lease_id: 41 }
        ))
    ));

    consume(&mut coordinator, rolled_back(40, 73));
    assert!(matches!(
        coordinator.state,
        TasControlState::RollbackPending { .. }
    ));

    consume(&mut coordinator, rolled_back(41, 73));
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn cancellation_waits_for_pending_acquire_then_rolls_back_exact_lease() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    assert!(coordinator.cancel().is_none());

    let response = consume(&mut coordinator, acquired(request_id, 8));

    bound_rollback(response, WORKER_GENERATION, 8);
    consume(&mut coordinator, rolled_back(8, 73));
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn project_close_cancellation_rolls_back_a_held_lease() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 12));

    let rollback = coordinator.cancel().unwrap();
    assert!(matches!(
        rollback.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::RollbackTasControl { lease_id: 12 }
        ))
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::RollbackPending { .. }
    ));
}

#[test]
fn rom_unload_and_shutdown_retirement_abandon_without_command() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 27));

    assert!(coordinator.retire_worker(WORKER_GENERATION));
    assert_eq!(coordinator.state, TasControlState::Detached);

    let mut coordinator = TasControlCoordinator::new();
    acquire(&mut coordinator);
    assert!(coordinator.retire_worker(WORKER_GENERATION));
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn command_rejection_is_returned_without_changing_authority() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 41));

    let response = consume(
        &mut coordinator,
        EmuResponse::TasControlCommandRejected {
            lease_id: 41,
            command: TasControlCommandKind::StateOrRecovery,
        },
    );

    assert!(matches!(
        response,
        ResponseDisposition::Unrelated(EmuResponse::TasControlCommandRejected {
            lease_id: 41,
            command: TasControlCommandKind::StateOrRecovery,
        })
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::ExecutionPending {
            worker_generation: WORKER_GENERATION,
            lease_id: 41,
            proof,
            ..
        } if proof.frame_count == 73
    ));
}

#[test]
fn matched_rollback_rejection_enters_terminal_fence_until_matching_worker_retires() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 41));
    let rollback = coordinator.cancel().unwrap();
    assert!(matches!(
        rollback.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::RollbackTasControl { lease_id: 41 }
        ))
    ));

    consume(
        &mut coordinator,
        EmuResponse::TasControlRollbackRejected {
            requested_lease_id: 41,
            reason: TasControlRollbackRejectedReason::RestoreFailed,
        },
    );

    assert!(!coordinator.gameplay_commands_allowed());
    assert!(coordinator.cancel().is_none());
    assert!(matches!(
        coordinator.state,
        TasControlState::Terminal {
            worker_generation: WORKER_GENERATION,
            reason: TasControlTerminalReason::RollbackRejected,
        }
    ));
    assert!(!coordinator.retire_worker(WORKER_GENERATION + 1));
    assert!(matches!(
        coordinator.state,
        TasControlState::Terminal { .. }
    ));

    assert!(coordinator.retire_worker(WORKER_GENERATION));
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn commit_is_token_bound_and_detaches_only_after_matching_success() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 51));
    complete_execution(&mut coordinator, 51, 1);
    let current = snapshot(0, "main", 0);
    let commit = coordinator.commit(Some(&current)).unwrap();
    assert!(matches!(
        commit.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::CommitTasControl { lease_id: 51 }
        ))
    ));

    consume(
        &mut coordinator,
        EmuResponse::TasControlCommitted { lease_id: 50 },
    );
    assert!(matches!(
        coordinator.state,
        TasControlState::CommitPending { lease_id: 51, .. }
    ));
    consume(
        &mut coordinator,
        EmuResponse::TasControlCommitted { lease_id: 51 },
    );
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn matched_commit_rejection_terminalizes_until_retirement() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 52));
    complete_execution(&mut coordinator, 52, 1);
    let current = snapshot(0, "main", 0);
    coordinator.commit(Some(&current)).unwrap();

    consume(
        &mut coordinator,
        EmuResponse::TasControlCommitRejected {
            requested_lease_id: 52,
            reason: TasControlCommitRejectedReason::NoActiveLease,
        },
    );
    assert!(matches!(
        coordinator.state,
        TasControlState::Terminal {
            worker_generation: WORKER_GENERATION,
            reason: TasControlTerminalReason::CommitRejected,
        }
    ));
    assert!(coordinator.retire_worker(WORKER_GENERATION));
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn rollback_success_with_wrong_checkpoint_proof_terminalizes() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 53));
    coordinator.cancel().unwrap();

    consume(
        &mut coordinator,
        EmuResponse::TasControlRolledBack {
            lease_id: 53,
            restored_state_sha256: crate::tas_project::TasDigest([8; 32]),
            frame_count: 73,
        },
    );
    assert!(matches!(
        coordinator.state,
        TasControlState::Terminal {
            worker_generation: WORKER_GENERATION,
            reason: TasControlTerminalReason::RollbackResponseMismatch,
        }
    ));
}

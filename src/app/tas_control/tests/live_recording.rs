use std::path::Path;

use super::*;
use crate::emu_backend::loader::DirectNesTasExecutionLoader;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasDigest, TasEditorSession, TasInputFrame,
    TasSeekStateCache,
};

fn live_session(root: &Path) -> TasEditorSession {
    let manual_path = root.join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.join("seek-cache")).unwrap();
    let source_path = root.join("game.nes");
    std::fs::write(&source_path, crate::test_support::build_nes_test_rom()).unwrap();
    let mut project = DirectNesTasExecutionLoader::new(source_path, Vec::new())
        .create_project()
        .unwrap();
    project
        .edit_transaction(|edit| edit.insert_frames("main", 1, 2))
        .unwrap();
    TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap()
}

fn replay_ready_602() -> (
    crate::test_support::TestDirectory,
    TasEditorSession,
    TasControlCoordinator,
    TasEditorControlSnapshot,
    u64,
) {
    let directory = crate::test_support::test_directory("tas-control-staged-replay").unwrap();
    let mut session = live_session(directory.path());
    session
        .edit_transaction(|edit| edit.insert_frames("main", 3, 599))
        .unwrap();
    session.set_cursor(601).unwrap();
    let project = TasEditorControlSnapshot::capture(&session).unwrap();
    let mut coordinator = TasControlCoordinator::new();
    let EmuCommand::AcquireTasControl { request_id, .. } = coordinator
        .begin_acquire(WORKER_GENERATION, project.clone())
        .unwrap()
    else {
        unreachable!();
    };
    let response = coordinator.consume_response_with_session(
        WORKER_GENERATION,
        acquired(request_id, 91),
        Some(TasAcquiredProjectBinding {
            snapshot: project.clone(),
            start_state_bytes: vec![0xAA],
            input_prefix: vec![crate::emu_thread::TasInputFrame::default(); 600],
            total_input_frames: 602,
        }),
        Some(&project),
        Some(&session),
    );
    let ResponseDisposition::Consumed {
        follow_up: Some(command),
    } = response
    else {
        panic!("acquisition should enqueue the initial bounded execution");
    };
    let Some((_, EmuCommand::ExecuteTasControl(request))) =
        command.into_parts_for_worker(WORKER_GENERATION)
    else {
        panic!("expected initial execution command");
    };
    let run_id = request.run_id;
    assert_eq!(request.input_prefix.len(), 600);

    assert!(matches!(
        coordinator.consume_response_with_session(
            WORKER_GENERATION,
            EmuResponse::TasExecutionCompleted {
                profile: TasExecutionProfile::DirectNesCartridge,
                lease_id: 91,
                run_id,
                segment_id: 1,
                segment_frame_count: 600,
                executed_project_frames: 600,
                frame_count: 19,
                state_sha256: TasDigest([0x31; 32]),
            },
            None,
            Some(&project),
            Some(&session),
        ),
        ResponseDisposition::ContinueExecutionReplay
    ));
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Staging {
            completed: 600,
            total: 602,
        }
    );

    let ResponseDisposition::Consumed {
        follow_up: Some(command),
    } = coordinator.continue_execution_replay(Some(&session))
    else {
        panic!("replay should enqueue one proof-bound frame");
    };
    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::AdvanceTasControl(request)
        )) if request.lease_id == 91
            && request.run_id == run_id
            && request.advance_id == 1
            && request.segment_id == 2
            && request.expected_segment_frame_count == 600
            && request.expected_executed_project_frames == 600
    ));
    (directory, session, coordinator, project, run_id)
}

#[test]
fn staging_over_600_reaches_awaiting_decision_with_bounded_segments() {
    let (_directory, session, mut coordinator, _project, run_id) = replay_ready_602();
    coordinator.start_mode = TasControlStartMode::Record;
    assert!(!coordinator.take_realtime_recording_start_request());
    assert!(matches!(
        coordinator.consume_execution_replay_advanced(
            WORKER_GENERATION,
            TasExecutionProfile::DirectNesCartridge,
            91,
            run_id,
            1,
            2,
            1,
            601,
            20,
            TasDigest([0x32; 32]),
            Some(&session),
        ),
        ResponseDisposition::ContinueExecutionReplay
    ));
    let ResponseDisposition::Consumed {
        follow_up: Some(command),
    } = coordinator.continue_execution_replay(Some(&session))
    else {
        panic!("replay should enqueue the final frame");
    };
    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::AdvanceTasControl(request)
        )) if request.advance_id == 2
            && request.segment_id == 2
            && request.expected_segment_frame_count == 1
            && request.expected_executed_project_frames == 601
    ));
    assert!(matches!(
        coordinator.consume_execution_replay_advanced(
            WORKER_GENERATION,
            TasExecutionProfile::DirectNesCartridge,
            91,
            run_id,
            2,
            2,
            2,
            602,
            21,
            TasDigest([0x33; 32]),
            Some(&session),
        ),
        ResponseDisposition::Consumed { follow_up: None }
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::AwaitingDecision {
            lease_id: 91,
            candidate_segment_id: 2,
            candidate_segment_frame_count: 2,
            candidate_executed_project_frames: 602,
            ..
        }
    ));
    assert!(coordinator.take_realtime_recording_start_request());
}

#[test]
fn stale_edit_during_staged_replay_rolls_back_the_original_checkpoint() {
    let (_directory, mut session, mut coordinator, _project, run_id) = replay_ready_602();

    session
        .edit_transaction(|edit| edit.insert_frames("main", 602, 1))
        .unwrap();
    let response = coordinator.consume_execution_replay_advanced(
        WORKER_GENERATION,
        TasExecutionProfile::DirectNesCartridge,
        91,
        run_id,
        1,
        2,
        1,
        601,
        20,
        TasDigest([0x32; 32]),
        Some(&session),
    );
    bound_rollback(response, WORKER_GENERATION, 91);
    assert!(matches!(
        coordinator.state,
        TasControlState::RollbackPending { lease_id: 91, .. }
    ));
}

fn awaiting_decision(coordinator: &mut TasControlCoordinator, lease_id: u64) -> u64 {
    let request_id = acquire(coordinator);
    consume(coordinator, acquired(request_id, lease_id));
    let run_id = match coordinator.state {
        TasControlState::ExecutionPending { run_id, .. } => run_id,
        _ => panic!("expected execution pending"),
    };
    complete_execution(coordinator, lease_id, run_id);
    assert!(coordinator.take_framebuffer_refresh());
    run_id
}

fn matching_advance(lease_id: u64, run_id: u64, advance_id: u64) -> EmuResponse {
    EmuResponse::TasFrameAdvanced {
        profile: TasExecutionProfile::DirectNesCartridge,
        lease_id,
        run_id,
        advance_id,
        segment_id: 1,
        segment_frame_count: advance_id + 1,
        executed_project_frames: advance_id + 1,
        frame_count: 20,
        state_sha256: crate::tas_project::TasDigest([0x42; 32]),
    }
}

#[test]
fn realtime_recording_starts_only_while_awaiting_a_decision_and_reports_its_state() {
    let mut coordinator = TasControlCoordinator::new();

    assert!(coordinator.start_realtime_recording().is_err());
    assert!(!coordinator.realtime_recording_active());

    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 70));
    assert!(coordinator.start_realtime_recording().is_err());
    assert!(!coordinator.realtime_recording_active());

    let run_id = match coordinator.state {
        TasControlState::ExecutionPending { run_id, .. } => run_id,
        _ => panic!("expected execution pending"),
    };
    complete_execution(&mut coordinator, 70, run_id);

    coordinator.start_realtime_recording().unwrap();
    assert!(coordinator.realtime_recording_active());
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Recording
    );

    coordinator.stop_realtime_recording();
    coordinator.stop_realtime_recording();
    assert!(!coordinator.realtime_recording_active());
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Linked {
            cursor: 1,
            recording_available: true,
        }
    );
}

#[test]
fn direct_gb_awaiting_decision_cannot_start_host_input_recording() {
    let mut coordinator = TasControlCoordinator::new();
    awaiting_decision(&mut coordinator, 77);
    let TasControlState::AwaitingDecision { project, .. } = &mut coordinator.state else {
        panic!("expected awaiting decision");
    };
    project.profile = crate::emu_thread::TasExecutionProfile::DirectGbRomOnlyDmg;

    assert!(!coordinator.can_record_live_input());
    assert!(coordinator.start_realtime_recording().is_err());
    assert!(!coordinator.realtime_recording_active());
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Linked {
            cursor: 1,
            recording_available: false,
        }
    );
}

#[test]
fn realtime_recording_has_no_total_prefix_limit() {
    let mut coordinator = TasControlCoordinator::new();
    awaiting_decision(&mut coordinator, 86);
    let TasControlState::AwaitingDecision {
        candidate_frame_count,
        ..
    } = &mut coordinator.state
    else {
        panic!("expected awaiting decision");
    };
    *candidate_frame_count = crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES + 10_000;

    coordinator.start_realtime_recording().unwrap();
    coordinator.stop_realtime_recording();

    let TasControlState::AwaitingDecision {
        project,
        candidate_segment_frame_count,
        candidate_executed_project_frames,
        ..
    } = &mut coordinator.state
    else {
        panic!("expected awaiting decision");
    };
    project.cursor = crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES - 1;
    *candidate_segment_frame_count = crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES;
    *candidate_executed_project_frames = crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES;

    coordinator.start_realtime_recording().unwrap();
    assert!(coordinator.realtime_recording_active());
}

#[test]
fn realtime_recording_survives_a_matched_frame_commit_and_can_chain() {
    let root = crate::test_support::test_directory("tas-realtime-record-chain").unwrap();
    let mut session = live_session(root.path());
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 80);
    coordinator.start_realtime_recording().unwrap();

    coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();
    assert!(coordinator.realtime_recording_active());
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Recording
    );

    let ResponseDisposition::CommitLiveFrame { prepared } = coordinator.consume_response(
        WORKER_GENERATION,
        matching_advance(80, run_id, 1),
        None,
        Some(&snapshot(0, "main", 0)),
    ) else {
        panic!("matching response should request an editor commit");
    };
    assert!(coordinator.realtime_recording_active());
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Recording
    );
    session.commit_prepared_live_frame(*prepared).unwrap();
    coordinator.finish_live_frame_commit(Ok(snapshot(1, "main", 1)));

    assert!(coordinator.realtime_recording_active());
    assert_eq!(
        coordinator.live_status(),
        crate::debug::TasEditorLiveStatus::Recording
    );
    let command = coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();
    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::AdvanceTasControl(request)
        )) if request.lease_id == 80 && request.run_id == run_id && request.advance_id == 2
    ));
}

#[test]
fn realtime_recording_crosses_the_segment_boundary_and_keeps_running() {
    let root = crate::test_support::test_directory("tas-realtime-record-limit-commit").unwrap();
    let mut session = live_session(root.path());
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 87);
    let TasControlState::AwaitingDecision {
        candidate_segment_frame_count,
        candidate_executed_project_frames,
        ..
    } = &mut coordinator.state
    else {
        panic!("expected awaiting decision");
    };
    *candidate_segment_frame_count = crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES;
    *candidate_executed_project_frames = crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES;
    coordinator.start_realtime_recording().unwrap();
    let command = coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();
    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::AdvanceTasControl(request)
        )) if request.segment_id == 2
            && request.expected_segment_frame_count
                == crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES
            && request.expected_executed_project_frames
                == crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES
    ));

    let ResponseDisposition::CommitLiveFrame { prepared } = coordinator.consume_response(
        WORKER_GENERATION,
        EmuResponse::TasFrameAdvanced {
            profile: TasExecutionProfile::DirectNesCartridge,
            lease_id: 87,
            run_id,
            advance_id: 1,
            segment_id: 2,
            segment_frame_count: 1,
            executed_project_frames: crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES + 1,
            frame_count: crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES,
            state_sha256: crate::tas_project::TasDigest([0x47; 32]),
        },
        None,
        Some(&snapshot(0, "main", 0)),
    ) else {
        panic!("matching response should request an editor commit");
    };
    session.commit_prepared_live_frame(*prepared).unwrap();
    coordinator.finish_live_frame_commit(Ok(snapshot(1, "main", 1)));

    assert!(coordinator.realtime_recording_active());
    assert!(matches!(
        coordinator.state,
        TasControlState::AwaitingDecision {
            candidate_segment_id: 2,
            candidate_segment_frame_count: 1,
            candidate_executed_project_frames,
            candidate_frame_count: crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES,
            ..
        } if candidate_executed_project_frames
            == crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES + 1
    ));
}

#[test]
fn stopping_realtime_recording_during_an_advance_allows_that_frame_to_commit() {
    let root = crate::test_support::test_directory("tas-realtime-record-stop-pending").unwrap();
    let mut session = live_session(root.path());
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 81);
    coordinator.start_realtime_recording().unwrap();
    coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();

    coordinator.stop_realtime_recording();
    coordinator.stop_realtime_recording();
    assert!(!coordinator.realtime_recording_active());

    let ResponseDisposition::CommitLiveFrame { prepared } = coordinator.consume_response(
        WORKER_GENERATION,
        matching_advance(81, run_id, 1),
        None,
        Some(&snapshot(0, "main", 0)),
    ) else {
        panic!("matching response should request an editor commit");
    };
    session.commit_prepared_live_frame(*prepared).unwrap();
    coordinator.finish_live_frame_commit(Ok(snapshot(1, "main", 1)));

    assert!(!coordinator.realtime_recording_active());
    assert!(matches!(
        coordinator.state,
        TasControlState::AwaitingDecision { .. }
    ));
}

#[test]
fn realtime_recording_clears_for_commit_project_divergence_terminalization_and_retirement() {
    let mut coordinator = TasControlCoordinator::new();
    awaiting_decision(&mut coordinator, 82);
    coordinator.start_realtime_recording().unwrap();
    coordinator.commit(Some(&snapshot(0, "main", 0))).unwrap();
    assert!(!coordinator.realtime_recording_active());

    let mut coordinator = TasControlCoordinator::new();
    awaiting_decision(&mut coordinator, 83);
    coordinator.start_realtime_recording().unwrap();
    assert!(
        coordinator
            .reconcile_project(Some(&snapshot(1, "main", 0)))
            .is_some()
    );
    assert!(!coordinator.realtime_recording_active());

    let mut coordinator = TasControlCoordinator::new();
    awaiting_decision(&mut coordinator, 84);
    coordinator.start_realtime_recording().unwrap();
    assert!(
        coordinator.terminalize_worker(WORKER_GENERATION, TasControlTerminalReason::RuntimeFault)
    );
    assert!(!coordinator.realtime_recording_active());

    let mut coordinator = TasControlCoordinator::new();
    awaiting_decision(&mut coordinator, 85);
    coordinator.start_realtime_recording().unwrap();
    assert!(coordinator.retire_worker(WORKER_GENERATION));
    assert!(!coordinator.realtime_recording_active());
}

#[test]
fn matched_live_record_commits_only_after_the_exact_worker_response() {
    let root = crate::test_support::test_directory("tas-live-record-matched").unwrap();
    let mut session = live_session(root.path());
    let before = session.project().encode().unwrap();
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 71);
    let prepared = session
        .prepare_live_frame(TasInputFrame {
            players: [
                crate::tas_project::TasControllerInput {
                    buttons: 3,
                    dpad: 4,
                },
                crate::tas_project::TasControllerInput {
                    buttons: 5,
                    dpad: 6,
                },
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
                crate::tas_project::TasControllerInput::default(),
            ],
            ..TasInputFrame::default()
        })
        .unwrap();
    let command = coordinator.begin_live_frame_advance(prepared).unwrap();
    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::AdvanceTasControl(request)
        )) if request.lease_id == 71
            && request.run_id == run_id
            && request.advance_id == 1
            && request.expected_frame_count == 19
            && request.expected_state_sha256 == crate::tas_project::TasDigest([0x31; 32])
            && request.input.p1_buttons == 3
            && request.input.p1_dpad == 4
            && request.input.p2_buttons == 5
            && request.input.p2_dpad == 6
    ));
    assert_eq!(session.project().encode().unwrap(), before);

    let current = snapshot(0, "main", 0);
    let ResponseDisposition::CommitLiveFrame { prepared } = coordinator.consume_response(
        WORKER_GENERATION,
        matching_advance(71, run_id, 1),
        None,
        Some(&current),
    ) else {
        panic!("matching response should release the prepared editor candidate");
    };
    assert_eq!(session.project().encode().unwrap(), before);
    session.commit_prepared_live_frame(*prepared).unwrap();
    let response = coordinator.finish_live_frame_commit(Ok(snapshot(1, "main", 1)));

    assert!(matches!(
        response,
        ResponseDisposition::Consumed { follow_up: None }
    ));
    assert!(coordinator.take_framebuffer_refresh());
    assert!(matches!(
        coordinator.state,
        TasControlState::AwaitingDecision {
            lease_id: 71,
            run_id: actual_run_id,
            next_advance_id: 2,
            candidate_frame_count: 20,
            candidate_state_sha256,
            project: TasEditorControlSnapshot {
                edit_generation: 1,
                cursor: 1,
                ..
            },
            ..
        } if actual_run_id == run_id
            && candidate_state_sha256 == crate::tas_project::TasDigest([0x42; 32])
    ));
}

#[test]
fn wrong_live_record_tokens_terminalize_without_editor_mutation() {
    let root = crate::test_support::test_directory("tas-live-record-wrong-token").unwrap();
    let session = live_session(root.path());
    let before = session.project().encode().unwrap();
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 72);
    coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();

    let current = snapshot(0, "main", 0);
    let response = coordinator.consume_response(
        WORKER_GENERATION,
        matching_advance(72, run_id, 2),
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
            reason: TasControlTerminalReason::FrameAdvanceResponseMismatch,
            ..
        }
    ));
    assert_eq!(session.project().encode().unwrap(), before);
}

#[test]
fn only_one_live_record_can_be_in_flight() {
    let root = crate::test_support::test_directory("tas-live-record-exclusive").unwrap();
    let session = live_session(root.path());
    let mut coordinator = TasControlCoordinator::new();
    awaiting_decision(&mut coordinator, 73);
    coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();

    assert!(
        coordinator
            .begin_live_frame_advance(
                session
                    .prepare_live_frame(TasInputFrame::default())
                    .unwrap()
            )
            .is_err()
    );
    assert!(matches!(
        coordinator.state,
        TasControlState::FrameAdvancePending {
            lease_id: 73,
            advance_id: 1,
            ..
        }
    ));
}

#[test]
fn cancellation_drops_the_prepared_live_record_without_mutating_the_editor() {
    let root = crate::test_support::test_directory("tas-live-record-cancel").unwrap();
    let session = live_session(root.path());
    let before = session.project().encode().unwrap();
    let mut coordinator = TasControlCoordinator::new();
    awaiting_decision(&mut coordinator, 74);
    coordinator.start_realtime_recording().unwrap();
    coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();

    let rollback = coordinator.cancel().unwrap();

    assert!(!coordinator.realtime_recording_active());
    assert!(matches!(
        rollback.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::RollbackTasControl { lease_id: 74 }
        ))
    ));
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.cursor(), 0);
}

#[test]
fn late_advanced_response_after_cancellation_is_stale_until_the_rollback_detaches() {
    let root = crate::test_support::test_directory("tas-live-record-late-after-cancel").unwrap();
    let session = live_session(root.path());
    let before = session.project().encode().unwrap();
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 76);
    coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();
    coordinator.cancel().unwrap();
    let current = snapshot(0, "main", 0);

    let response = coordinator.consume_response(
        WORKER_GENERATION,
        matching_advance(76, run_id, 1),
        None,
        Some(&current),
    );

    assert!(matches!(
        response,
        ResponseDisposition::Consumed { follow_up: None }
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::RollbackPending { lease_id: 76, .. }
    ));
    assert_eq!(session.project().encode().unwrap(), before);
    consume(&mut coordinator, rolled_back(76, 73));
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn non_authority_live_record_rejection_rolls_back_without_editor_mutation() {
    let root = crate::test_support::test_directory("tas-live-record-rejected").unwrap();
    let session = live_session(root.path());
    let before = session.project().encode().unwrap();
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 77);
    coordinator.start_realtime_recording().unwrap();
    coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();

    let response = coordinator.consume_response(
        WORKER_GENERATION,
        EmuResponse::TasFrameAdvanceRejected {
            profile: TasExecutionProfile::DirectNesCartridge,
            requested_lease_id: 77,
            run_id,
            advance_id: 1,
            segment_id: 1,
            reason: crate::emu_thread::TasFrameAdvanceRejectedReason::FrameProgressFailed,
        },
        None,
        Some(&snapshot(0, "main", 0)),
    );

    bound_rollback(response, WORKER_GENERATION, 77);
    assert!(!coordinator.realtime_recording_active());
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.cursor(), 0);
}

#[test]
fn each_new_live_run_starts_frame_advance_ids_at_one() {
    let root = crate::test_support::test_directory("tas-live-record-fresh-id").unwrap();
    let mut session = live_session(root.path());
    let mut coordinator = TasControlCoordinator::new();
    let first_run_id = awaiting_decision(&mut coordinator, 78);
    coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();
    let ResponseDisposition::CommitLiveFrame { prepared } = coordinator.consume_response(
        WORKER_GENERATION,
        matching_advance(78, first_run_id, 1),
        None,
        Some(&snapshot(0, "main", 0)),
    ) else {
        panic!("first advance should request an editor commit");
    };
    session.commit_prepared_live_frame(*prepared).unwrap();
    coordinator.finish_live_frame_commit(Ok(snapshot(1, "main", 1)));
    coordinator.commit(Some(&snapshot(1, "main", 1))).unwrap();
    consume(
        &mut coordinator,
        EmuResponse::TasControlCommitted { lease_id: 78 },
    );
    assert_eq!(coordinator.state, TasControlState::Detached);

    let fresh_session = live_session(root.path());
    let second_run_id = awaiting_decision(&mut coordinator, 79);
    let command = coordinator
        .begin_live_frame_advance(
            fresh_session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();

    assert!(matches!(
        command.into_parts_for_worker(WORKER_GENERATION),
        Some((
            WORKER_GENERATION,
            EmuCommand::AdvanceTasControl(request)
        )) if request.lease_id == 79 && request.run_id == second_run_id && request.advance_id == 1
    ));
}

#[test]
fn editor_commit_failure_rolls_back_without_refreshing_or_mutating_the_editor() {
    let root = crate::test_support::test_directory("tas-live-record-editor-failure").unwrap();
    let session = live_session(root.path());
    let before = session.project().encode().unwrap();
    let mut coordinator = TasControlCoordinator::new();
    let run_id = awaiting_decision(&mut coordinator, 75);
    coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();
    let current = snapshot(0, "main", 0);
    let ResponseDisposition::CommitLiveFrame { prepared: _ } = coordinator.consume_response(
        WORKER_GENERATION,
        matching_advance(75, run_id, 1),
        None,
        Some(&current),
    ) else {
        panic!("matching response should request the editor commit");
    };

    let response = coordinator.finish_live_frame_commit(Err(anyhow::anyhow!("stale editor")));

    bound_rollback(response, WORKER_GENERATION, 75);
    assert!(matches!(
        coordinator.state,
        TasControlState::RollbackPending { lease_id: 75, .. }
    ));
    assert!(!coordinator.take_framebuffer_refresh());
    assert_eq!(session.project().encode().unwrap(), before);
    assert_eq!(session.cursor(), 0);
}

#[test]
fn command_loss_releases_pending_frame_for_history_group_cleanup() {
    let root = crate::test_support::test_directory("tas-live-record-command-loss").unwrap();
    let mut session = live_session(root.path());
    session.begin_live_recording_history_group().unwrap();
    let mut coordinator = TasControlCoordinator::new();
    awaiting_decision(&mut coordinator, 88);
    coordinator.start_realtime_recording().unwrap();
    coordinator
        .begin_live_frame_advance(
            session
                .prepare_live_frame(TasInputFrame::default())
                .unwrap(),
        )
        .unwrap();

    assert!(coordinator.terminalize_worker(
        WORKER_GENERATION,
        TasControlTerminalReason::CommandChannelClosed,
    ));

    assert!(!coordinator.realtime_recording_active());
    assert!(!coordinator.live_frame_in_flight());
    assert!(!session.end_live_recording_history_group().unwrap());
}

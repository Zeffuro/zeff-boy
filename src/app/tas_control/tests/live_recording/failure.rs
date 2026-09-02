use super::*;

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
    assert_eq!(session.cursor(), 1);
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
    assert_eq!(session.cursor(), 1);
}

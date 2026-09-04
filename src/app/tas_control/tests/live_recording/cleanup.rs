use super::*;

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
    let ResponseDisposition::CommitLiveFrame { prepared, .. } = coordinator.consume_response(
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
    let ResponseDisposition::CommitLiveFrame { .. } = coordinator.consume_response(
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
    assert_eq!(session.cursor(), 1);
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

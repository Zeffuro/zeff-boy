use super::super::support::tas_nes_test_loop;
use super::{acquire, advance_request, advance_request_in_segment, completed_proof, request};
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionRejectedReason as Rejected,
    TasFrameAdvanceRejectedReason as AdvanceRejected, TasFrameAdvanceSnapshot, TasInputFrame,
};
use crate::tas_project::TasDigest;

mod advance_failures;
mod cache;
mod segments;
mod zapper;

#[test]
fn direct_nes_run_executes_exact_prefix_and_rollback_restores_checkpoint() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let checkpoint = emu_loop.backend.encode_state_bytes().unwrap();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let input_prefix = vec![
        TasInputFrame {
            p1_buttons: 1,
            p2_dpad: 2,
            ..Default::default()
        },
        TasInputFrame {
            p1_dpad: 4,
            p2_buttons: 8,
            ..Default::default()
        },
    ];
    let (mut expected_loop, _) = tas_nes_test_loop();
    expected_loop
        .backend
        .load_state_from_bytes(start_state.clone())
        .unwrap();
    for input in &input_prefix {
        expected_loop
            .backend
            .apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
                buttons: input.p1_buttons,
                dpad: input.p1_dpad,
                buttons_p2: input.p2_buttons,
                dpad_p2: input.p2_dpad,
                ..Default::default()
            });
        expected_loop.backend.step_frame();
    }
    let expected_state = expected_loop.backend.encode_state_bytes().unwrap();

    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, input_prefix)));
    let candidate_sha256 = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            lease_id: actual_lease,
            run_id: 1,
            frame_count: 2,
            state_sha256,
            ..
        } if actual_lease == lease_id => state_sha256,
        _ => panic!("unexpected execution response"),
    };
    assert_eq!(emu_loop.backend.frame_count(), 2);
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected_state
    );
    assert_eq!(
        candidate_sha256,
        TasDigest::from_bytes(&emu_loop.backend.encode_state_bytes().unwrap())
    );
    assert_eq!(
        emu_loop.shared_framebuffer.load_full().unwrap().as_slice(),
        emu_loop.backend.framebuffer(),
    );

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack {
            lease_id: actual_lease,
            frame_count: 0,
            ..
        } if actual_lease == lease_id
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

#[test]
fn direct_nes_zero_boundary_restores_start_state_and_completes() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    emu_loop.backend.step_frame();
    assert_ne!(emu_loop.backend.encode_state_bytes().unwrap(), start_state);

    assert!(emu_loop.handle_command(request(lease_id, 1, start_state.clone(), Vec::new(),)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            lease_id: actual_lease_id,
            run_id: 1,
            segment_id: 1,
            segment_frame_count: 0,
            executed_project_frames: 0,
            frame_count: 0,
            state_sha256,
            ..
        } if actual_lease_id == lease_id && state_sha256 == TasDigest::from_bytes(&start_state)
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), start_state);
}

#[test]
fn silent_positioning_audio_does_not_leak_into_the_next_live_frame() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let prefix = vec![TasInputFrame::default(); 8];
    let live_input = TasInputFrame {
        p1_buttons: 1,
        ..TasInputFrame::default()
    };

    let (mut expected_loop, _) = tas_nes_test_loop();
    expected_loop
        .backend
        .load_state_from_bytes(start_state.clone())
        .unwrap();
    for input in &prefix {
        expected_loop
            .backend
            .apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
                buttons: input.p1_buttons,
                dpad: input.p1_dpad,
                buttons_p2: input.p2_buttons,
                dpad_p2: input.p2_dpad,
                ..Default::default()
            });
        expected_loop.backend.step_frame();
    }
    let mut silent_positioning_audio = Vec::new();
    expected_loop
        .backend
        .drain_audio_samples_into(&mut silent_positioning_audio);
    assert!(!silent_positioning_audio.is_empty());
    expected_loop
        .backend
        .apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: live_input.p1_buttons,
            dpad: live_input.p1_dpad,
            buttons_p2: live_input.p2_buttons,
            dpad_p2: live_input.p2_dpad,
            ..Default::default()
        });
    expected_loop.backend.step_frame();
    let mut expected_live_audio = Vec::new();
    expected_loop
        .backend
        .drain_audio_samples_into(&mut expected_live_audio);

    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, prefix,)));
    let (frame_count, state_sha256) = completed_proof(responses.recv().unwrap(), lease_id, 1);
    assert!(emu_loop.handle_command(advance_request(
        lease_id,
        1,
        1,
        frame_count,
        state_sha256,
        live_input,
    )));
    let actual_live_audio = match responses.recv().unwrap() {
        EmuResponse::TasFrameAdvanced { audio_samples, .. } => audio_samples,
        _ => panic!("unexpected frame-advance response"),
    };

    assert_eq!(actual_live_audio, expected_live_audio);
}

#[test]
fn accepted_live_frame_captures_the_requested_debug_snapshot() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        start_state,
        vec![TasInputFrame::default()],
    )));
    let (frame_count, state_sha256) = completed_proof(responses.recv().unwrap(), lease_id, 1);
    let mut frame_input = super::super::support::frame_input(0);
    frame_input.snapshot.want_debug_info = true;
    let mut command = advance_request(
        lease_id,
        1,
        1,
        frame_count,
        state_sha256,
        TasInputFrame::default(),
    );
    let EmuCommand::AdvanceTasControl(request) = &mut command else {
        unreachable!();
    };
    request.snapshot = Some(TasFrameAdvanceSnapshot {
        request: frame_input.snapshot,
        buffers: frame_input.buffers,
    });

    assert!(emu_loop.handle_command(command));
    match responses.recv().unwrap() {
        EmuResponse::TasFrameAdvanced {
            ui_data: Some(ui_data),
            ..
        } => assert!(ui_data.cpu_debug.is_some()),
        _ => panic!("expected a TAS frame response with debug data"),
    }
}

#[test]
fn repeated_exact_boundary_uses_worker_cache_and_rollback_keeps_original_checkpoint() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let checkpoint = emu_loop.backend.encode_state_bytes().unwrap();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let input = vec![TasInputFrame {
        p1_buttons: 0x01,
        ..TasInputFrame::default()
    }];

    assert!(emu_loop.handle_command(request(lease_id, 1, start_state.clone(), input.clone(),)));
    let first_state_sha256 = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            run_id: 1,
            segment_frame_count: 1,
            executed_project_frames: 1,
            state_sha256,
            ..
        } => state_sha256,
        _ => panic!("unexpected first execution response"),
    };

    assert!(emu_loop.handle_command(request(lease_id, 2, start_state, input)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            run_id: 2,
            segment_id: 1,
            segment_frame_count: 0,
            executed_project_frames: 1,
            state_sha256,
            ..
        } if state_sha256 == first_state_sha256
    ));

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { .. }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

#[test]
fn execution_rejects_a_cache_proof_that_does_not_match_the_bounded_prefix_shape() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let mut command = request(lease_id, 1, start_state, vec![TasInputFrame::default()]);
    let EmuCommand::ExecuteTasControl(request) = &mut command else {
        unreachable!();
    };
    request.cache_proof.target_cursor = 2;

    assert!(emu_loop.handle_command(command));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            reason: Rejected::InvalidCacheProof,
            ..
        }
    ));
    assert!(emu_loop.tas_control.is_leased());
}

#[test]
fn zapper_runtime_can_stage_standard_project_and_restore_or_commit() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let project_start = emu_loop.backend.encode_state_bytes().unwrap();
    emu_loop
        .backend
        .set_zapper_state(true, false, false, Some((12, 34)));
    let zapper_checkpoint = emu_loop.backend.encode_state_bytes().unwrap();
    assert_eq!(
        emu_loop
            .backend
            .nes_has_standard_or_zapper_controller_topology(),
        Some(true)
    );
    assert_eq!(
        emu_loop.backend.nes_has_standard_controller_topology(),
        Some(false)
    );

    let (lease_id, witness_state) = acquire(&mut emu_loop, &responses);
    assert_eq!(witness_state, zapper_checkpoint);
    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        project_start.clone(),
        vec![Default::default()],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted { .. }
    ));
    assert_eq!(
        emu_loop.backend.nes_has_standard_controller_topology(),
        Some(true)
    );
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { .. }
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        zapper_checkpoint
    );
    assert_eq!(
        emu_loop.backend.nes_has_standard_controller_topology(),
        Some(false)
    );

    let (lease_id, _) = acquire(&mut emu_loop, &responses);
    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        project_start,
        vec![Default::default()],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted { .. }
    ));
    assert!(emu_loop.handle_command(EmuCommand::CommitTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlCommitted { .. }
    ));
    assert_eq!(
        emu_loop.backend.nes_has_standard_controller_topology(),
        Some(true)
    );
}

#[test]
fn repeated_runs_require_exact_next_ids_and_replace_or_clear_the_candidate() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let before = emu_loop.backend.encode_state_bytes().unwrap();
    assert!(emu_loop.handle_command(request(
        lease_id + 1,
        1,
        start_state.clone(),
        vec![Default::default()],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            reason: Rejected::WrongLease { active_lease_id },
            ..
        } if active_lease_id == lease_id
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);

    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        start_state.clone(),
        vec![Default::default()],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted { run_id: 1, .. }
    ));
    let first_candidate = emu_loop.backend.encode_state_bytes().unwrap();
    assert!(emu_loop.handle_command(request(
        lease_id,
        3,
        start_state.clone(),
        vec![Default::default()],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            reason: Rejected::RunAlreadyAttempted { active_run_id: 1 },
            ..
        }
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        first_candidate
    );

    assert!(emu_loop.handle_command(request(
        lease_id,
        2,
        start_state,
        vec![TasInputFrame {
            p1_buttons: 1,
            ..Default::default()
        }]
    )));
    let (frame_count, state_sha256) = completed_proof(responses.recv().unwrap(), lease_id, 2);
    assert!(emu_loop.handle_command(advance_request(
        lease_id,
        2,
        1,
        frame_count,
        state_sha256,
        TasInputFrame::default(),
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanced {
            run_id: 2,
            advance_id: 1,
            segment_id: 1,
            segment_frame_count: 2,
            ..
        }
    ));
    let replacement_candidate = emu_loop.backend.encode_state_bytes().unwrap();
    assert!(emu_loop.handle_command(EmuCommand::CommitTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlCommitted { lease_id: actual } if actual == lease_id
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        replacement_candidate
    );
}

#[test]
fn failed_repeated_run_poison_requires_rollback() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let checkpoint = emu_loop.backend.encode_state_bytes().unwrap();
    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        start_state.clone(),
        vec![Default::default()],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted { run_id: 1, .. }
    ));

    assert!(emu_loop.handle_command(request(lease_id, 2, Vec::new(), vec![Default::default()],)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            run_id: 2,
            reason: Rejected::InvalidStartState,
            ..
        }
    ));
    assert!(emu_loop.handle_command(EmuCommand::CommitTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlCommitRejected {
            reason: crate::emu_thread::TasControlCommitRejectedReason::NoCompletedExecution,
            ..
        }
    ));
    assert!(emu_loop.handle_command(request(lease_id, 3, start_state, vec![Default::default()],)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            reason: Rejected::RunAlreadyAttempted { active_run_id: 2 },
            ..
        }
    ));

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { .. }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

#[test]
fn malformed_execution_is_typed_and_remains_leased_for_rollback() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, _) = acquire(&mut emu_loop, &responses);
    assert!(emu_loop.handle_command(request(lease_id, 1, Vec::new(), vec![Default::default()],)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            requested_lease_id,
            run_id: 1,
            reason: Rejected::InvalidStartState,
            ..
        } if requested_lease_id == lease_id
    ));
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { lease_id: actual, .. } if actual == lease_id
    ));
}

#[test]
fn initial_execution_remains_bounded_to_one_segment() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        start_state,
        vec![Default::default(); crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES as usize + 1],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            requested_lease_id,
            run_id: 1,
            reason: Rejected::FrameLimitExceeded,
            ..
        } if requested_lease_id == lease_id
    ));
    assert!(emu_loop.tas_control.is_leased());
}

#[test]
fn commit_rejects_a_tampered_candidate_without_releasing_the_lease() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, vec![Default::default()],)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted { run_id: 1, .. }
    ));
    emu_loop.backend.step_frame();

    assert!(emu_loop.handle_command(EmuCommand::CommitTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlCommitRejected {
            reason: crate::emu_thread::TasControlCommitRejectedReason::CandidateStateDigestMismatch,
            ..
        }
    ));
    assert!(emu_loop.tas_control.is_leased());
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { lease_id: actual, .. } if actual == lease_id
    ));
}

#[test]
fn one_frame_advance_updates_exact_candidate_and_commit_preserves_it() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let run_id = 1;
    let first_input = TasInputFrame {
        p1_buttons: 1,
        p2_dpad: 2,
        ..Default::default()
    };
    let next_input = TasInputFrame {
        p1_dpad: 4,
        p2_buttons: 8,
        ..Default::default()
    };
    let (mut expected_loop, _) = tas_nes_test_loop();
    expected_loop
        .backend
        .load_state_from_bytes(start_state.clone())
        .unwrap();
    for input in [first_input, next_input] {
        expected_loop
            .backend
            .apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
                buttons: input.p1_buttons,
                dpad: input.p1_dpad,
                buttons_p2: input.p2_buttons,
                dpad_p2: input.p2_dpad,
                ..Default::default()
            });
        expected_loop.backend.step_frame();
    }
    let expected_state = expected_loop.backend.encode_state_bytes().unwrap();

    assert!(emu_loop.handle_command(request(lease_id, run_id, start_state, vec![first_input],)));
    let (frame_count, state_sha256) = completed_proof(responses.recv().unwrap(), lease_id, run_id);
    assert!(emu_loop.handle_command(advance_request(
        lease_id,
        run_id,
        1,
        frame_count,
        state_sha256,
        next_input,
    )));
    let (advanced_sha256, audio_samples) = match responses.recv().unwrap() {
        EmuResponse::TasFrameAdvanced {
            lease_id: actual_lease_id,
            run_id: actual_run_id,
            advance_id: 1,
            frame_count: 2,
            state_sha256,
            audio_samples,
            ..
        } if actual_lease_id == lease_id && actual_run_id == run_id => {
            (state_sha256, audio_samples)
        }
        _ => panic!("unexpected frame-advance response"),
    };
    assert!(!audio_samples.is_empty());
    assert!(emu_loop.tas_control.is_leased());
    assert_eq!(emu_loop.backend.frame_count(), 2);
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected_state
    );
    assert_eq!(advanced_sha256, TasDigest::from_bytes(&expected_state));
    assert_eq!(
        emu_loop.shared_framebuffer.load_full().unwrap().as_slice(),
        emu_loop.backend.framebuffer(),
    );

    let third_input = TasInputFrame {
        p1_buttons: 0x10,
        p2_dpad: 0x20,
        ..Default::default()
    };
    expected_loop
        .backend
        .apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: third_input.p1_buttons,
            dpad: third_input.p1_dpad,
            buttons_p2: third_input.p2_buttons,
            dpad_p2: third_input.p2_dpad,
            ..Default::default()
        });
    expected_loop.backend.step_frame();
    let expected_final_state = expected_loop.backend.encode_state_bytes().unwrap();
    assert!(emu_loop.handle_command(advance_request(
        lease_id,
        run_id,
        2,
        2,
        advanced_sha256,
        third_input,
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanced {
            lease_id: actual_lease_id,
            run_id: actual_run_id,
            advance_id: 2,
            frame_count: 3,
            state_sha256,
            ..
        } if actual_lease_id == lease_id
            && actual_run_id == run_id
            && state_sha256 == TasDigest::from_bytes(&expected_final_state)
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected_final_state
    );

    assert!(emu_loop.handle_command(EmuCommand::CommitTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlCommitted { lease_id: actual } if actual == lease_id
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected_final_state
    );
}

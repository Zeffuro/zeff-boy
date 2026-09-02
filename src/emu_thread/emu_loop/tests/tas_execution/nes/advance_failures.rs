use super::*;

#[test]
fn frame_advance_rejects_wrong_tokens_proof_and_nonsequential_ids_without_mutation() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let run_id = 1;
    assert!(emu_loop.handle_command(request(
        lease_id,
        run_id,
        start_state,
        vec![Default::default()],
    )));
    let (frame_count, state_sha256) = completed_proof(responses.recv().unwrap(), lease_id, run_id);
    let candidate = emu_loop.backend.encode_state_bytes().unwrap();
    let cases = [
        (
            advance_request(
                lease_id + 1,
                run_id,
                1,
                frame_count,
                state_sha256,
                Default::default(),
            ),
            AdvanceRejected::WrongLease {
                active_lease_id: lease_id,
            },
            lease_id + 1,
            run_id,
            1,
        ),
        (
            advance_request(
                lease_id,
                run_id + 1,
                1,
                frame_count,
                state_sha256,
                Default::default(),
            ),
            AdvanceRejected::WrongRun {
                active_run_id: run_id,
            },
            lease_id,
            run_id + 1,
            1,
        ),
        (
            advance_request(
                lease_id,
                run_id,
                0,
                frame_count,
                state_sha256,
                Default::default(),
            ),
            AdvanceRejected::InvalidAdvanceId,
            lease_id,
            run_id,
            0,
        ),
        (
            advance_request(
                lease_id,
                run_id,
                2,
                frame_count,
                state_sha256,
                Default::default(),
            ),
            AdvanceRejected::UnexpectedAdvanceId {
                expected_advance_id: 1,
            },
            lease_id,
            run_id,
            2,
        ),
        (
            advance_request_in_segment(
                lease_id,
                run_id,
                1,
                (1, 1, 1),
                (frame_count + 1, state_sha256),
                Default::default(),
            ),
            AdvanceRejected::CandidateProofMismatch,
            lease_id,
            run_id,
            1,
        ),
        (
            advance_request_in_segment(
                lease_id,
                run_id,
                1,
                (2, 1, 1),
                (frame_count, state_sha256),
                Default::default(),
            ),
            AdvanceRejected::UnexpectedSegmentId {
                expected_segment_id: 1,
            },
            lease_id,
            run_id,
            1,
        ),
        (
            advance_request_in_segment(
                lease_id,
                run_id,
                1,
                (1, 0, 1),
                (frame_count, state_sha256),
                Default::default(),
            ),
            AdvanceRejected::SegmentProofMismatch,
            lease_id,
            run_id,
            1,
        ),
        (
            advance_request_in_segment(
                lease_id,
                run_id,
                1,
                (1, 1, 2),
                (frame_count, state_sha256),
                Default::default(),
            ),
            AdvanceRejected::SegmentProofMismatch,
            lease_id,
            run_id,
            1,
        ),
    ];
    for (command, expected_reason, expected_lease_id, expected_run_id, expected_advance_id) in cases
    {
        assert!(emu_loop.handle_command(command));
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::TasFrameAdvanceRejected {
                requested_lease_id,
                run_id,
                advance_id,
                reason,
                ..
            } if requested_lease_id == expected_lease_id
                && run_id == expected_run_id
                && advance_id == expected_advance_id
                && reason == expected_reason
        ));
        assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), candidate);
    }

    assert!(emu_loop.handle_command(advance_request(
        lease_id,
        run_id,
        1,
        frame_count,
        state_sha256,
        Default::default(),
    )));
    let (advanced_frame_count, advanced_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasFrameAdvanced {
            frame_count,
            state_sha256,
            ..
        } => (frame_count, state_sha256),
        _ => panic!("unexpected frame-advance response"),
    };
    let advanced = emu_loop.backend.encode_state_bytes().unwrap();
    for advance_id in [1, 3] {
        assert!(emu_loop.handle_command(advance_request(
            lease_id,
            run_id,
            advance_id,
            advanced_frame_count,
            advanced_sha256,
            Default::default(),
        )));
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::TasFrameAdvanceRejected {
                reason: AdvanceRejected::UnexpectedAdvanceId {
                    expected_advance_id: 2
                },
                ..
            }
        ));
        assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), advanced);
    }
}

#[test]
fn frame_advance_requires_an_existing_completed_candidate() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let digest = TasDigest::from_bytes(&[]);
    assert!(emu_loop.handle_command(advance_request(1, 1, 1, 0, digest, Default::default(),)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanceRejected {
            reason: AdvanceRejected::NoActiveLease,
            ..
        }
    ));

    let (lease_id, _) = acquire(&mut emu_loop, &responses);
    assert!(emu_loop.handle_command(advance_request(
        lease_id,
        1,
        1,
        0,
        digest,
        Default::default(),
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanceRejected {
            reason: AdvanceRejected::NoCompletedExecution,
            ..
        }
    ));
    assert!(emu_loop.tas_control.is_leased());
}

#[test]
fn candidate_tampering_rejects_advance_and_rollback_restores_checkpoint() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let checkpoint = emu_loop.backend.encode_state_bytes().unwrap();
    let checkpoint_sha256 = TasDigest::from_bytes(&checkpoint);
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let run_id = 1;
    assert!(emu_loop.handle_command(request(
        lease_id,
        run_id,
        start_state,
        vec![Default::default()],
    )));
    let (frame_count, state_sha256) = completed_proof(responses.recv().unwrap(), lease_id, run_id);
    assert!(emu_loop.handle_command(advance_request(
        lease_id,
        run_id,
        1,
        frame_count,
        state_sha256,
        TasInputFrame {
            p1_buttons: 0x40,
            ..Default::default()
        },
    )));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasFrameAdvanced {
            advance_id: 1,
            frame_count,
            state_sha256,
            ..
        } => (frame_count, state_sha256),
        _ => panic!("unexpected frame-advance response"),
    };
    emu_loop.backend.step_frame();
    let tampered = emu_loop.backend.encode_state_bytes().unwrap();

    assert!(emu_loop.handle_command(advance_request(
        lease_id,
        run_id,
        2,
        frame_count,
        state_sha256,
        Default::default(),
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanceRejected {
            reason: AdvanceRejected::CandidateStateDigestMismatch,
            ..
        }
    ));
    assert!(emu_loop.tas_control.is_leased());
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), tampered);

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack {
            lease_id: actual_lease_id,
            restored_state_sha256,
            frame_count: 0,
        } if actual_lease_id == lease_id && restored_state_sha256 == checkpoint_sha256
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

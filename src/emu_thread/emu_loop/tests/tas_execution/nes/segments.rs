use super::*;

#[test]
fn frame_601_atomically_starts_segment_two_and_keeps_advance_ids_global() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let checkpoint = emu_loop.backend.encode_state_bytes().unwrap();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let run_id = 1;
    assert!(emu_loop.handle_command(request(
        lease_id,
        run_id,
        start_state,
        vec![Default::default(); crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES as usize],
    )));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            lease_id: actual_lease_id,
            run_id: actual_run_id,
            segment_id: 1,
            segment_frame_count,
            executed_project_frames,
            frame_count,
            state_sha256,
            ..
        } if actual_lease_id == lease_id
            && actual_run_id == run_id
            && segment_frame_count == crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES
            && executed_project_frames == crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES =>
        {
            (frame_count, state_sha256)
        }
        _ => panic!("unexpected execution response"),
    };
    let candidate = emu_loop.backend.encode_state_bytes().unwrap();

    assert!(emu_loop.handle_command(advance_request(
        lease_id,
        run_id,
        1,
        frame_count,
        state_sha256,
        Default::default(),
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanceRejected {
            reason: AdvanceRejected::UnexpectedSegmentId {
                expected_segment_id: 2,
            },
            ..
        }
    ));
    assert!(emu_loop.tas_control.is_leased());
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), candidate);

    assert!(emu_loop.handle_command(advance_request_in_segment(
        lease_id,
        run_id,
        1,
        (
            2,
            crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES,
            crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES,
        ),
        (frame_count, state_sha256),
        Default::default(),
    )));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasFrameAdvanced {
            lease_id: actual_lease_id,
            run_id: actual_run_id,
            advance_id: 1,
            segment_id: 2,
            segment_frame_count: 1,
            executed_project_frames,
            frame_count,
            state_sha256,
            ..
        } if actual_lease_id == lease_id
            && actual_run_id == run_id
            && executed_project_frames
                == crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES + 1 =>
        {
            (frame_count, state_sha256)
        }
        _ => panic!("unexpected frame-advance response"),
    };
    assert!(emu_loop.handle_command(advance_request_in_segment(
        lease_id,
        run_id,
        2,
        (
            2,
            1,
            crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES + 1,
        ),
        (frame_count, state_sha256),
        Default::default(),
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanced {
            advance_id: 2,
            segment_id: 2,
            segment_frame_count: 2,
            executed_project_frames,
            ..
        } if executed_project_frames
            == crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES + 2
    ));

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack {
            lease_id: actual_lease_id,
            ..
        } if actual_lease_id == lease_id
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

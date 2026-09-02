use super::{acquire, advance_request_in_segment, completed_proof, request, tas_nes_test_loop};
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionCacheProof, TasExecutionPredecessorWindow, TasInputFrame,
};
use crate::tas_project::TasDigest;

fn predecessor_request(
    lease_id: u64,
    run_id: u64,
    start_state: Vec<u8>,
    inputs: Vec<TasInputFrame>,
    source_cursors: &[u64],
) -> EmuCommand {
    let mut command = request(lease_id, run_id, start_state, inputs.clone());
    let EmuCommand::ExecuteTasControl(request) = &mut command else {
        unreachable!();
    };
    let input_start_cursor = source_cursors.iter().copied().min().unwrap();
    request.predecessor_window = Some(TasExecutionPredecessorWindow {
        source_proofs: source_cursors
            .iter()
            .copied()
            .map(|target_cursor| TasExecutionCacheProof {
                sync_identity_sha256: TasDigest([0x92; 32]),
                branch_prefix_sha256: super::super::synthetic_input_prefix_sha256(
                    &inputs[..target_cursor as usize],
                ),
                target_cursor,
            })
            .collect(),
        input_start_cursor,
        input_frames: inputs[input_start_cursor as usize..].to_vec(),
    });
    command
}

fn expected_state(start_state: &[u8], inputs: &[TasInputFrame]) -> Vec<u8> {
    let (mut expected, _) = tas_nes_test_loop();
    expected
        .backend
        .load_state_from_bytes(start_state.to_vec())
        .unwrap();
    for input in inputs {
        expected
            .backend
            .apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
                buttons: input.p1_buttons,
                dpad: input.p1_dpad,
                buttons_p2: input.p2_buttons,
                dpad_p2: input.p2_dpad,
                zapper: input.zapper,
                ..Default::default()
            });
        expected.backend.step_frame();
    }
    expected.backend.encode_state_bytes().unwrap()
}

#[test]
fn direct_nes_reuses_the_nearest_valid_predecessor_and_replays_only_the_suffix() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let original = vec![TasInputFrame::default(); 8];

    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        start_state.clone(),
        original[..4].to_vec(),
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            segment_frame_count: 4,
            executed_project_frames: 4,
            ..
        }
    ));

    assert!(emu_loop.handle_command(predecessor_request(
        lease_id,
        2,
        start_state.clone(),
        original[..6].to_vec(),
        &[6, 4],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            segment_frame_count: 2,
            executed_project_frames: 6,
            ..
        }
    ));

    let mut edited = original;
    edited[4].p1_buttons = 1;
    assert!(emu_loop.handle_command(predecessor_request(
        lease_id,
        3,
        start_state.clone(),
        edited.clone(),
        &[8, 6, 4],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            segment_frame_count: 4,
            executed_project_frames: 8,
            ..
        }
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected_state(&start_state, &edited)
    );
}

#[test]
fn long_seek_builds_an_intermediate_state_for_the_next_nearby_seek() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let inputs = vec![TasInputFrame::default(); 602];
    let mut first = request(lease_id, 1, start_state.clone(), inputs[..600].to_vec());
    let EmuCommand::ExecuteTasControl(first_request) = &mut first else {
        unreachable!();
    };
    first_request.cache_proof.target_cursor = 601;
    first_request.cache_proof.branch_prefix_sha256 =
        super::super::synthetic_input_prefix_sha256(&inputs[..601]);
    first_request.intermediate_cache_proofs = vec![TasExecutionCacheProof {
        sync_identity_sha256: TasDigest([0x92; 32]),
        branch_prefix_sha256: super::super::synthetic_input_prefix_sha256(&inputs[..600]),
        target_cursor: 600,
    }];

    assert!(emu_loop.handle_command(first));
    let (frame_count, state_sha256) = completed_proof(responses.recv().unwrap(), lease_id, 1);
    assert!(emu_loop.handle_command(advance_request_in_segment(
        lease_id,
        1,
        1,
        (2, 600, 600),
        (frame_count, state_sha256),
        inputs[600],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanced {
            executed_project_frames: 601,
            ..
        }
    ));

    let mut nearby = request(lease_id, 2, start_state.clone(), inputs[..600].to_vec());
    let EmuCommand::ExecuteTasControl(nearby_request) = &mut nearby else {
        unreachable!();
    };
    nearby_request.cache_proof.target_cursor = 602;
    nearby_request.cache_proof.branch_prefix_sha256 =
        super::super::synthetic_input_prefix_sha256(&inputs[..602]);
    nearby_request.intermediate_cache_proofs = vec![TasExecutionCacheProof {
        sync_identity_sha256: TasDigest([0x92; 32]),
        branch_prefix_sha256: super::super::synthetic_input_prefix_sha256(&inputs[..600]),
        target_cursor: 600,
    }];
    nearby_request.predecessor_window = Some(TasExecutionPredecessorWindow {
        source_proofs: vec![
            nearby_request.cache_proof,
            TasExecutionCacheProof {
                sync_identity_sha256: TasDigest([0x92; 32]),
                branch_prefix_sha256: super::super::synthetic_input_prefix_sha256(&inputs[..600]),
                target_cursor: 600,
            },
        ],
        input_start_cursor: 600,
        input_frames: inputs[600..602].to_vec(),
    });

    assert!(emu_loop.handle_command(nearby));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            run_id: 2,
            segment_frame_count: 2,
            executed_project_frames: 602,
            ..
        }
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected_state(&start_state, &inputs)
    );
}

#[test]
fn corrupt_predecessor_falls_back_to_the_start_without_installing_partial_state() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let inputs = vec![TasInputFrame::default(); 6];

    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        start_state.clone(),
        inputs[..4].to_vec(),
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted { .. }
    ));
    let source_proof = TasExecutionCacheProof {
        sync_identity_sha256: TasDigest([0x92; 32]),
        branch_prefix_sha256: super::super::synthetic_input_prefix_sha256(&inputs[..4]),
        target_cursor: 4,
    };
    assert!(
        emu_loop
            .tas_control
            .corrupt_cached_state_for_test(source_proof)
    );

    assert!(emu_loop.handle_command(predecessor_request(
        lease_id,
        2,
        start_state.clone(),
        inputs.clone(),
        &[6, 4],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            segment_frame_count: 6,
            executed_project_frames: 6,
            ..
        }
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected_state(&start_state, &inputs)
    );
}

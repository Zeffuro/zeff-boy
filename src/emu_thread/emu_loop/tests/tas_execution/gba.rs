use super::super::support::tas_nes_test_loop_from_backend;
use crate::emu_backend::loader::DirectGbaTasExecutionLoader;
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionCacheProof, TasExecutionProfile,
    TasExecutionRejectedReason, TasExecutionRequest, TasFrameAdvanceRejectedReason,
    TasFrameAdvanceRequest, TasInputFrame,
};
use crate::tas_project::TasDigest;

fn gba_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom
}

fn loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectGbaTasExecutionLoader,
    crate::tas_project::TasProject,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let path = directory.path().join("game.gba");
    std::fs::write(&path, gba_rom()).unwrap();
    let loader = DirectGbaTasExecutionLoader::new(path);
    let project = loader.create_project().unwrap();
    (directory, loader, project)
}

fn request(
    lease_id: u64,
    run_id: u64,
    start_state_bytes: Vec<u8>,
    input_prefix: Vec<TasInputFrame>,
) -> EmuCommand {
    EmuCommand::ExecuteTasControl(Box::new(TasExecutionRequest {
        profile: TasExecutionProfile::DirectGbaCartridge,
        lease_id,
        run_id,
        intermediate_cache_proofs: Vec::new(),
        cache_proof: TasExecutionCacheProof {
            sync_identity_sha256: TasDigest([0x47; 32]),
            branch_prefix_sha256: TasDigest([run_id as u8; 32]),
            target_cursor: input_prefix.len() as u64,
        },
        predecessor_window: None,
        start_state_bytes,
        input_prefix,
    }))
}

#[test]
fn direct_gba_worker_executes_advances_and_rolls_back_one_pad_input() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-gba");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 108,
        profile: TasExecutionProfile::DirectGbaCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 108,
            lease_id,
            witness,
        } => {
            assert_eq!(witness.profile, TasExecutionProfile::DirectGbaCartridge);
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let input = TasInputFrame {
        p1_buttons: 0x31,
        p1_dpad: 0x04,
        ..Default::default()
    };
    expected.set_input(input.p1_buttons, input.p1_dpad);
    expected.step_frame();
    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, vec![input])));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectGbaCartridge,
            lease_id: actual_lease_id,
            run_id: 1,
            frame_count,
            state_sha256,
            ..
        } if actual_lease_id == lease_id => (frame_count, state_sha256),
        _ => panic!("unexpected execution response"),
    };
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );

    let next_input = TasInputFrame {
        p1_buttons: 0x02,
        p1_dpad: 0x01,
        ..Default::default()
    };
    expected.set_input(next_input.p1_buttons, next_input.p1_dpad);
    expected.step_frame();
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectGbaCartridge,
                lease_id,
                run_id: 1,
                advance_id: 1,
                segment_id: 1,
                expected_segment_frame_count: 1,
                expected_executed_project_frames: 1,
                expected_frame_count: frame_count,
                expected_state_sha256: state_sha256,
                input: next_input,
                snapshot: None,
            },
        )))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanced {
            profile: TasExecutionProfile::DirectGbaCartridge,
            frame_count: 2,
            ..
        }
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );

    let nonneutral_state = emu_loop.backend.encode_state_bytes().unwrap();
    assert!(emu_loop.handle_command(request(lease_id, 2, nonneutral_state.clone(), Vec::new(),)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectGbaCartridge,
            lease_id: actual_lease_id,
            run_id: 2,
            frame_count: 2,
            ..
        } if actual_lease_id == lease_id
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        nonneutral_state
    );

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    match responses.recv().unwrap() {
        EmuResponse::TasControlRolledBack { frame_count: 0, .. } => {}
        EmuResponse::TasControlRolledBack { frame_count, .. } => {
            panic!("unexpected rollback frame count: {frame_count}")
        }
        EmuResponse::TasControlRollbackRejected { reason, .. } => {
            panic!("rollback rejected: {reason:?}")
        }
        _ => panic!("unexpected rollback response"),
    }
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

#[test]
fn direct_gba_frame_advance_rejects_same_frame_state_divergence() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-gba-divergence");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 110,
        profile: TasExecutionProfile::DirectGbaCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, vec![Default::default()],)));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            lease_id: actual_lease_id,
            run_id: 1,
            frame_count,
            state_sha256,
            ..
        } if actual_lease_id == lease_id => (frame_count, state_sha256),
        _ => panic!("unexpected execution response"),
    };

    emu_loop.backend.set_input(0x20, 0x08);
    let diverged = emu_loop.backend.encode_state_bytes().unwrap();
    assert_eq!(emu_loop.backend.frame_count(), frame_count);
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectGbaCartridge,
                lease_id,
                run_id: 1,
                advance_id: 1,
                segment_id: 1,
                expected_segment_frame_count: 1,
                expected_executed_project_frames: 1,
                expected_frame_count: frame_count,
                expected_state_sha256: state_sha256,
                input: Default::default(),
                snapshot: None,
            },
        )))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanceRejected {
            reason: TasFrameAdvanceRejectedReason::CandidateStateDigestMismatch,
            ..
        }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), diverged);
}

#[test]
fn direct_gba_worker_rejects_unowned_input_without_mutation() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-gba-invalid");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 109,
        profile: TasExecutionProfile::DirectGbaCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        start_state,
        vec![TasInputFrame {
            p2_buttons: 1,
            ..Default::default()
        }],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            reason: TasExecutionRejectedReason::InvalidInput,
            ..
        }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

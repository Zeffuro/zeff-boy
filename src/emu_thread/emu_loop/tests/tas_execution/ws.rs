use super::super::support::tas_nes_test_loop_from_backend;
use crate::emu_backend::loader::DirectWsTasExecutionLoader;
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionCacheProof, TasExecutionPredecessorWindow,
    TasExecutionProfile, TasExecutionRejectedReason, TasExecutionRequest, TasFrameAdvanceRequest,
    TasInputFrame,
};
use crate::tas_project::TasDigest;

fn ws_rom() -> Vec<u8> {
    let mut rom = vec![0x90; 128 * 1024];
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer..].fill(0);
    rom[footer + 1] = 1;
    rom[footer + 4] = 0x01;
    rom[footer + 5] = 0;
    rom[footer + 6] = 1;
    rom[footer + 7] = 0;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn ws_rtc_rom() -> Vec<u8> {
    let mut rom = ws_rom();
    let footer = rom.len() - 10;
    rom[footer + 7] = 1;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectWsTasExecutionLoader,
    crate::tas_project::TasProject,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let path = directory.path().join("game.wsc");
    std::fs::write(&path, ws_rom()).unwrap();
    let loader = DirectWsTasExecutionLoader::new(path);
    let project = loader.create_project().unwrap();
    (directory, loader, project)
}

#[test]
fn direct_ws_rtc_worker_link_acquisition_is_rejected() {
    let directory = crate::test_support::test_directory("tas-control-direct-ws-rtc").unwrap();
    let path = directory.path().join("clock.wsc");
    std::fs::write(&path, ws_rtc_rom()).unwrap();
    let loader = DirectWsTasExecutionLoader::new(path);
    let project = loader.create_project().unwrap();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 104,
        profile: TasExecutionProfile::DirectWsCartridge,
    }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 104,
            ..
        }
    ));
}

fn request(
    lease_id: u64,
    run_id: u64,
    start_state_bytes: Vec<u8>,
    input_prefix: Vec<TasInputFrame>,
) -> EmuCommand {
    let target_cursor = input_prefix.len() as u64;
    EmuCommand::ExecuteTasControl(Box::new(TasExecutionRequest {
        profile: TasExecutionProfile::DirectWsCartridge,
        lease_id,
        run_id,
        intermediate_cache_proofs: Vec::new(),
        cache_proof: TasExecutionCacheProof {
            sync_identity_sha256: TasDigest([0x57; 32]),
            branch_prefix_sha256: TasDigest([run_id as u8; 32]),
            target_cursor,
        },
        predecessor_window: None,
        start_state_bytes,
        input_prefix,
    }))
}

#[test]
fn direct_ws_worker_executes_advances_and_rolls_back_exact_keypad_input() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-ws");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 102,
        profile: TasExecutionProfile::DirectWsCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 102,
            lease_id,
            witness,
        } => {
            assert_eq!(witness.profile, TasExecutionProfile::DirectWsCartridge);
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let input = TasInputFrame {
        p1_buttons: 0x81,
        p1_dpad: 0x04,
        ..Default::default()
    };
    expected.set_input(input.p1_buttons, input.p1_dpad);
    expected.step_frame();

    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, vec![input])));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectWsCartridge,
            lease_id: actual_lease_id,
            run_id: 1,
            frame_count,
            state_sha256,
            ..
        } if actual_lease_id == lease_id => (frame_count, state_sha256),
        EmuResponse::TasExecutionRejected { reason, .. } => {
            panic!("unexpected execution rejection: {reason:?}")
        }
        _ => panic!("unexpected execution response"),
    };
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );

    let next_input = TasInputFrame {
        p1_buttons: 0x12,
        p1_dpad: 0x01,
        ..Default::default()
    };
    expected.set_input(next_input.p1_buttons, next_input.p1_dpad);
    expected.step_frame();
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectWsCartridge,
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
            profile: TasExecutionProfile::DirectWsCartridge,
            frame_count: 2,
            ..
        }
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { frame_count: 0, .. }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

#[test]
fn direct_ws_worker_reuses_a_predecessor_cache_and_rejects_unowned_input() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-ws-cache");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 103,
        profile: TasExecutionProfile::DirectWsCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    let inputs = vec![
        TasInputFrame {
            p1_buttons: 0x01,
            ..Default::default()
        },
        TasInputFrame {
            p1_buttons: 0x80,
            p1_dpad: 0x08,
            ..Default::default()
        },
    ];
    let first = request(lease_id, 1, start_state.clone(), inputs[..1].to_vec());
    let EmuCommand::ExecuteTasControl(first_request) = &first else {
        unreachable!()
    };
    let first_proof = first_request.cache_proof;
    assert!(emu_loop.handle_command(first));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            segment_frame_count: 1,
            executed_project_frames: 1,
            ..
        }
    ));

    let mut second = request(lease_id, 2, start_state.clone(), inputs.clone());
    let EmuCommand::ExecuteTasControl(second_request) = &mut second else {
        unreachable!()
    };
    second_request.predecessor_window = Some(TasExecutionPredecessorWindow {
        source_proofs: vec![second_request.cache_proof, first_proof],
        input_start_cursor: 1,
        input_frames: inputs[1..].to_vec(),
    });
    assert!(emu_loop.handle_command(second));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            segment_frame_count: 1,
            executed_project_frames: 2,
            ..
        }
    ));
    for input in inputs {
        expected.set_input(input.p1_buttons, input.p1_dpad);
        expected.step_frame();
    }
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { .. }
    ));
    let invalid = TasInputFrame {
        p2_buttons: 1,
        ..Default::default()
    };
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 104,
        profile: TasExecutionProfile::DirectWsCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, vec![invalid])));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            reason: TasExecutionRejectedReason::InvalidInput,
            ..
        }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

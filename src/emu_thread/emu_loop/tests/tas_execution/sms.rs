use super::super::support::tas_nes_test_loop_from_backend;
use crate::emu_backend::loader::DirectSmsTasExecutionLoader;
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionCacheProof, TasExecutionProfile,
    TasExecutionRejectedReason, TasExecutionRequest, TasFrameAdvanceRequest, TasInputFrame,
};
use crate::tas_project::TasDigest;

fn codemasters_rom() -> Vec<u8> {
    let offset = zeff_sega8_core::hardware::constants::CODEMASTERS_HEADER_OFFSET;
    let mut rom = vec![0xFF; offset + 16];
    rom[offset] = 2;
    rom[offset + 1..offset + 6].copy_from_slice(&[0x31, 0x08, 0x93, 0x10, 0x59]);
    rom[offset + 6..offset + 8].copy_from_slice(&0x1234u16.to_le_bytes());
    rom[offset + 8..offset + 10].copy_from_slice(&0xEDCCu16.to_le_bytes());
    rom[offset + 10..offset + 16].fill(0);
    rom
}

fn loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectSmsTasExecutionLoader,
    crate::tas_project::TasProject,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let path = directory.path().join("game.sms");
    std::fs::write(&path, codemasters_rom()).unwrap();
    let loader = DirectSmsTasExecutionLoader::new(path);
    let project = loader.create_project().unwrap();
    (directory, loader, project)
}

fn request(
    lease_id: u64,
    run_id: u64,
    start_state_bytes: Vec<u8>,
    input_prefix: Vec<TasInputFrame>,
) -> EmuCommand {
    let target_cursor = input_prefix.len() as u64;
    EmuCommand::ExecuteTasControl(Box::new(TasExecutionRequest {
        profile: TasExecutionProfile::DirectSmsCartridge,
        lease_id,
        run_id,
        intermediate_cache_proofs: Vec::new(),
        cache_proof: TasExecutionCacheProof {
            sync_identity_sha256: TasDigest([0x53; 32]),
            branch_prefix_sha256: TasDigest([0x4D; 32]),
            target_cursor,
        },
        predecessor_window: None,
        start_state_bytes,
        input_prefix,
    }))
}

#[test]
fn direct_sms_worker_executes_advances_and_rolls_back_exact_two_pad_input() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-sms");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 92,
        profile: TasExecutionProfile::DirectSmsCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 92,
            lease_id,
            witness,
        } => {
            assert_eq!(witness.profile, TasExecutionProfile::DirectSmsCartridge);
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let input = TasInputFrame {
        p1_buttons: 0x01,
        p1_dpad: 0x04,
        p2_buttons: 0x02,
        p2_dpad: 0x08,
        ..Default::default()
    };
    expected.set_input(input.p1_buttons, input.p1_dpad);
    expected.set_input_p2(input.p2_buttons, input.p2_dpad);
    expected.step_frame();

    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, vec![input])));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectSmsCartridge,
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
        p2_dpad: 0x02,
        ..Default::default()
    };
    expected.set_input(next_input.p1_buttons, next_input.p1_dpad);
    expected.set_input_p2(next_input.p2_buttons, next_input.p2_dpad);
    expected.step_frame();
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectSmsCartridge,
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
            profile: TasExecutionProfile::DirectSmsCartridge,
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
fn direct_sms_worker_rejects_non_sms_input_without_mutation() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-sms-input");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 93,
        profile: TasExecutionProfile::DirectSmsCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    let invalid = TasInputFrame {
        p1_buttons: 0x04,
        ..Default::default()
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

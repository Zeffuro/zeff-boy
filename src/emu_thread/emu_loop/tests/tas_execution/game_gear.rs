use super::super::support::tas_nes_test_loop_from_backend;
use crate::emu_backend::loader::DirectGameGearTasExecutionLoader;
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionCacheProof, TasExecutionPredecessorWindow,
    TasExecutionProfile, TasExecutionRejectedReason, TasExecutionRequest, TasFrameAdvanceRequest,
    TasInputFrame,
};
use crate::tas_project::TasDigest;
use zeff_sega8_core::hardware::cartridge::{GameGearCartridgeIdentity, GameGearStandardMapperRam};

fn loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectGameGearTasExecutionLoader,
    crate::tas_project::TasProject,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let path = directory.path().join("game.gg");
    let mut rom = vec![0x00; 16 * 1024];
    let offset = 0x3FF0;
    rom[offset..offset + 8].copy_from_slice(b"TMR SEGA");
    rom[offset + 0x0A..offset + 0x0C].copy_from_slice(&0x1234u16.to_le_bytes());
    rom[offset + 0x0C] = 0x42;
    rom[offset + 0x0D] = 0x31;
    rom[offset + 0x0E] = 0xA5;
    rom[offset + 0x0F] = 0x6A;
    std::fs::write(&path, &rom).unwrap();
    let loader = DirectGameGearTasExecutionLoader::new_with_catalog_entry(
        path,
        GameGearCartridgeIdentity {
            sha256: zeff_firmware::sha256_bytes(&rom),
            source_len: rom.len(),
        },
        GameGearStandardMapperRam::Absent,
    );
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
        profile: TasExecutionProfile::DirectGameGearCartridge,
        lease_id,
        run_id,
        intermediate_cache_proofs: Vec::new(),
        cache_proof: TasExecutionCacheProof {
            sync_identity_sha256: TasDigest([0x47; 32]),
            branch_prefix_sha256: TasDigest([0x47; 32]),
            target_cursor: input_prefix.len() as u64,
        },
        predecessor_window: None,
        start_state_bytes,
        input_prefix,
    }))
}

#[test]
fn direct_game_gear_worker_executes_advances_rejects_p2_and_rolls_back() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-game-gear");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 94,
        profile: TasExecutionProfile::DirectGameGearCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => {
            assert_eq!(
                witness.profile,
                TasExecutionProfile::DirectGameGearCartridge
            );
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let input = TasInputFrame {
        p1_buttons: 0x09,
        p1_dpad: 0x04,
        ..Default::default()
    };
    let first = request(lease_id, 1, start_state.clone(), vec![input]);
    let EmuCommand::ExecuteTasControl(first_request) = &first else {
        unreachable!()
    };
    let first_proof = first_request.cache_proof;
    assert!(emu_loop.handle_command(first));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectGameGearCartridge,
            segment_frame_count: 1,
            executed_project_frames: 1,
            ..
        }
    ));
    let second_input = TasInputFrame {
        p1_buttons: 0x02,
        p1_dpad: 0x01,
        ..Default::default()
    };
    let mut second = request(lease_id, 2, start_state, vec![input, second_input]);
    let EmuCommand::ExecuteTasControl(second_request) = &mut second else {
        unreachable!()
    };
    second_request.predecessor_window = Some(TasExecutionPredecessorWindow {
        source_proofs: vec![second_request.cache_proof, first_proof],
        input_start_cursor: 1,
        input_frames: vec![second_input],
    });
    assert!(emu_loop.handle_command(second));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectGameGearCartridge,
            frame_count,
            state_sha256,
            segment_frame_count: 1,
            executed_project_frames: 2,
            ..
        } => (frame_count, state_sha256),
        _ => panic!("unexpected cached execution response"),
    };
    let inspection = zeff_sega8_core::save_state::inspect_current_native_game_gear_tas_state(
        &emu_loop.backend.sega8().unwrap().emu,
        &emu_loop.backend.encode_state_bytes().unwrap(),
    )
    .unwrap();
    assert!(!inspection.start_pressed);

    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectGameGearCartridge,
                lease_id,
                run_id: 2,
                advance_id: 1,
                segment_id: 1,
                expected_segment_frame_count: 1,
                expected_executed_project_frames: 2,
                expected_frame_count: frame_count,
                expected_state_sha256: state_sha256,
                input: TasInputFrame {
                    p1_buttons: 0x01,
                    p1_dpad: 0x08,
                    ..Default::default()
                },
                snapshot: None,
            },
        )))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanced {
            profile: TasExecutionProfile::DirectGameGearCartridge,
            frame_count: 3,
            ..
        }
    ));
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { frame_count: 0, .. }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 95,
        profile: TasExecutionProfile::DirectGameGearCartridge,
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

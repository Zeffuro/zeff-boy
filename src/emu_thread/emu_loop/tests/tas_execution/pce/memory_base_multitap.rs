use super::*;

#[test]
fn direct_pce_cd_memory_base_multitap_worker_reuses_cache_and_rolls_back() {
    let directory =
        crate::test_support::test_directory("tas-control-pce-cd-memory-base-multitap").unwrap();
    let source_path = directory.path().join("disc.cue");
    let mut disc = vec![0xE5; 4 * 2048];
    disc[0..4].copy_from_slice(&[0x4D, 0x42, 0xE5, 0x51]);
    std::fs::write(directory.path().join("disc.bin"), disc).unwrap();
    std::fs::write(
        &source_path,
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )
    .unwrap();
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        firmware_sha256,
    );
    let disc_sha256 = base
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256);
    let _controller_catalog =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            disc_sha256,
            zeff_pce_core::hardware::PceControllerMode::Multitap,
        );
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        source_path,
        system_card,
        firmware_sha256,
    );
    let project = loader.create_project().unwrap();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let pce = backend.pce().unwrap();
    assert_eq!(
        pce.memory_base_mode(),
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled
    );
    assert_eq!(
        pce.arcade_card_mode(),
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled
    );
    let checkpoint = backend.encode_state_bytes().unwrap();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 141,
        profile: TasExecutionProfile::DirectPceMultitapCd,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => {
            assert_eq!(witness.profile, TasExecutionProfile::DirectPceMultitapCd);
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let inputs = vec![
        TasInputFrame {
            p1_buttons: 1,
            p2_buttons: 2,
            p3_buttons: 4,
            p4_buttons: 8,
            p5_dpad: 1,
            ..Default::default()
        },
        TasInputFrame {
            p1_dpad: 8,
            p2_dpad: 4,
            p3_dpad: 2,
            p4_dpad: 1,
            p5_buttons: 8,
            ..Default::default()
        },
    ];
    let first_proof = TasExecutionCacheProof {
        sync_identity_sha256: TasDigest([0x77; 32]),
        branch_prefix_sha256: TasDigest([0x78; 32]),
        target_cursor: 1,
    };
    assert!(
        emu_loop.handle_command(EmuCommand::ExecuteTasControl(Box::new(
            TasExecutionRequest {
                profile: TasExecutionProfile::DirectPceMultitapCd,
                lease_id,
                run_id: 1,
                intermediate_cache_proofs: Vec::new(),
                cache_proof: first_proof,
                predecessor_window: None,
                start_state_bytes: start_state.clone(),
                input_prefix: inputs[..1].to_vec(),
            },
        )))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            segment_frame_count: 1,
            executed_project_frames: 1,
            ..
        }
    ));

    let second_proof = TasExecutionCacheProof {
        sync_identity_sha256: TasDigest([0x77; 32]),
        branch_prefix_sha256: TasDigest([0x79; 32]),
        target_cursor: 2,
    };
    assert!(
        emu_loop.handle_command(EmuCommand::ExecuteTasControl(Box::new(
            TasExecutionRequest {
                profile: TasExecutionProfile::DirectPceMultitapCd,
                lease_id,
                run_id: 2,
                intermediate_cache_proofs: Vec::new(),
                cache_proof: second_proof,
                predecessor_window: Some(crate::emu_thread::TasExecutionPredecessorWindow {
                    source_proofs: vec![second_proof, first_proof],
                    input_start_cursor: 1,
                    input_frames: inputs[1..].to_vec(),
                }),
                start_state_bytes: start_state,
                input_prefix: inputs.clone(),
            },
        )))
    );
    let state_sha256 = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectPceMultitapCd,
            segment_frame_count: 1,
            executed_project_frames: 2,
            state_sha256,
            ..
        } => state_sha256,
        _ => panic!("unexpected execution response"),
    };
    let state_bytes = emu_loop.backend.encode_state_bytes().unwrap();
    assert_eq!(state_sha256, TasDigest::from_bytes(&state_bytes));
    let mut residual_audio = Vec::new();
    emu_loop
        .backend
        .drain_audio_samples_into(&mut residual_audio);
    assert!(residual_audio.is_empty());
    for input in inputs {
        expected.set_input(input.p1_buttons, input.p1_dpad);
        expected.set_input_p2(input.p2_buttons, input.p2_dpad);
        expected.set_input_p3(input.p3_buttons, input.p3_dpad);
        expected.set_input_p4(input.p4_buttons, input.p4_dpad);
        expected.set_input_p5(input.p5_buttons, input.p5_dpad);
        expected.step_frame();
        expected.drain_audio_samples_into(&mut Vec::new());
    }
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

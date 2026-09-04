use super::*;

#[test]
fn direct_pce_cd_arcade_multitap_worker_executes_five_ports_and_rolls_back() {
    let directory =
        crate::test_support::test_directory("tas-control-pce-cd-arcade-multitap").unwrap();
    let source_path = directory.path().join("disc.cue");
    let mut disc = vec![0xDB; 4 * 2048];
    disc[0..4].copy_from_slice(&[0x41, 0x4D, 0xDB, 0x24]);
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
    let _arcade_catalog =
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256);
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
    assert_eq!(
        backend.pce().unwrap().arcade_card_mode(),
        zeff_pce_core::hardware::PceArcadeCardMode::Enabled
    );
    let checkpoint = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 134,
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
    assert!(
        emu_loop.handle_command(EmuCommand::ExecuteTasControl(Box::new(
            TasExecutionRequest {
                profile: TasExecutionProfile::DirectPceMultitapCd,
                lease_id,
                run_id: 1,
                intermediate_cache_proofs: Vec::new(),
                cache_proof: TasExecutionCacheProof {
                    sync_identity_sha256: TasDigest([0x75; 32]),
                    branch_prefix_sha256: TasDigest([0x76; 32]),
                    target_cursor: 1,
                },
                predecessor_window: None,
                start_state_bytes: start_state,
                input_prefix: vec![TasInputFrame {
                    p1_buttons: 1,
                    p2_buttons: 2,
                    p3_buttons: 4,
                    p4_buttons: 8,
                    p5_dpad: 1,
                    ..Default::default()
                }],
            },
        )))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectPceMultitapCd,
            frame_count: 1,
            ..
        }
    ));
    assert_ne!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { frame_count: 0, .. }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

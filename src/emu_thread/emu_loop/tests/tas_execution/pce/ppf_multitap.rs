use super::*;

#[test]
fn direct_pce_cd_ppf_multitap_worker_executes_five_ports_and_rolls_back() {
    let directory = crate::test_support::test_directory("tas-control-pce-cd-ppf-multitap").unwrap();
    let source_path = directory.path().join("disc.cue");
    std::fs::write(directory.path().join("disc.bin"), vec![0xD7; 4 * 2048]).unwrap();
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
    let source_disc_sha256 = base
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        source_disc_sha256,
        zeff_pce_core::hardware::PceControllerMode::Multitap,
    );
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &source_path,
        vec![("worker.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )
    .unwrap();
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_and_ppf_stack(
        source_path,
        system_card,
        firmware_sha256,
        stack,
    );
    let project = loader.create_project().unwrap();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 133,
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
                    sync_identity_sha256: TasDigest([0x73; 32]),
                    branch_prefix_sha256: TasDigest([0x74; 32]),
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
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { frame_count: 0, .. }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

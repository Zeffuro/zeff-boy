use super::super::support::tas_nes_test_loop_from_backend;
use crate::emu_backend::loader::{DirectPceCdTasExecutionLoader, DirectPceTasExecutionLoader};
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionCacheProof, TasExecutionProfile,
    TasExecutionRejectedReason, TasExecutionRequest, TasFrameAdvanceRequest, TasInputFrame,
};
use crate::tas_project::TasDigest;

fn loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectPceTasExecutionLoader,
    crate::tas_project::TasProject,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let path = directory.path().join("game.pce");
    let mut rom = vec![0; 512];
    rom[0] = 1;
    rom.extend(vec![0xEA; 0x2000]);
    std::fs::write(&path, rom).unwrap();
    let loader = DirectPceTasExecutionLoader::new(path);
    let project = loader.create_project().unwrap();
    (directory, loader, project)
}

fn six_button_loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectPceTasExecutionLoader,
    crate::tas_project::TasProject,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let path = directory.path().join("game.pce");
    let mut rom = vec![0; 512];
    rom[0] = 1;
    rom.extend(vec![0xEA; 0x2000]);
    std::fs::write(&path, rom).unwrap();
    let loader = DirectPceTasExecutionLoader::new_six_button(path);
    let project = loader.create_project().unwrap();
    (directory, loader, project)
}

fn chd_loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::tas_project::TasProject,
    crate::emu_backend::pce_profiles::TestMemoryBaseCatalogGuard,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let chd_path = directory.path().join("disc.chd");
    crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&chd_path).unwrap();
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let system_card_sha256 = zeff_firmware::sha256_bytes(system_card);
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        chd_path,
        system_card,
        system_card_sha256,
    );
    let normalized_disc_sha256 = loader
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            normalized_disc_sha256,
        );
    let project = loader.create_project().unwrap();
    (directory, loader, project, memory_base_catalog)
}

fn iso_loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::tas_project::TasProject,
    crate::emu_backend::pce_profiles::TestMemoryBaseCatalogGuard,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let iso_path = directory.path().join("disc.iso");
    std::fs::write(&iso_path, vec![0x5A; 4 * 2048]).unwrap();
    std::fs::write(
        directory.path().join("disc.cue"),
        b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )
    .unwrap();
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let system_card_sha256 = zeff_firmware::sha256_bytes(system_card);
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        iso_path,
        system_card,
        system_card_sha256,
    );
    let normalized_disc_sha256 = loader
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            normalized_disc_sha256,
        );
    let project = loader.create_project().unwrap();
    (directory, loader, project, memory_base_catalog)
}

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}

fn ppf_loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::tas_project::TasProject,
    crate::emu_backend::pce_profiles::TestMemoryBaseCatalogGuard,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let cue_path = directory.path().join("disc.cue");
    std::fs::write(directory.path().join("disc.bin"), vec![0x5A; 4 * 2048]).unwrap();
    std::fs::write(
        &cue_path,
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )
    .unwrap();
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let system_card_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base_loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        cue_path.clone(),
        system_card,
        system_card_sha256,
    );
    let source_disc_sha256 = base_loader
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            source_disc_sha256,
        );
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![("memory-base.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )
    .unwrap();
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path,
        system_card,
        system_card_sha256,
        stack,
    );
    let project = loader.create_project().unwrap();
    (directory, loader, project, memory_base_catalog)
}

fn request(
    lease_id: u64,
    run_id: u64,
    start_state_bytes: Vec<u8>,
    input_prefix: Vec<TasInputFrame>,
) -> EmuCommand {
    let target_cursor = input_prefix.len() as u64;
    EmuCommand::ExecuteTasControl(Box::new(TasExecutionRequest {
        profile: TasExecutionProfile::DirectPceHuCard,
        lease_id,
        run_id,
        intermediate_cache_proofs: Vec::new(),
        cache_proof: TasExecutionCacheProof {
            sync_identity_sha256: TasDigest([0x50; 32]),
            branch_prefix_sha256: TasDigest([run_id as u8; 32]),
            target_cursor,
        },
        predecessor_window: None,
        start_state_bytes,
        input_prefix,
    }))
}

#[test]
fn direct_pce_worker_executes_advances_rolls_back_and_rejects_unowned_input() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-pce");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 105,
        profile: TasExecutionProfile::DirectPceHuCard,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 105,
            lease_id,
            witness,
        } => {
            assert_eq!(witness.profile, TasExecutionProfile::DirectPceHuCard);
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let input = TasInputFrame {
        p1_buttons: 0x03,
        p1_dpad: 0x04,
        ..Default::default()
    };
    expected.set_input(input.p1_buttons, input.p1_dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());

    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, vec![input])));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectPceHuCard,
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
        p1_buttons: 0x08,
        p1_dpad: 0x01,
        ..Default::default()
    };
    expected.set_input(next_input.p1_buttons, next_input.p1_dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectPceHuCard,
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
            profile: TasExecutionProfile::DirectPceHuCard,
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

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 106,
        profile: TasExecutionProfile::DirectPceHuCard,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    let invalid = TasInputFrame {
        p2_buttons: 1,
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

#[test]
fn direct_pce_six_button_worker_preserves_extra_buttons_and_rolls_back() {
    let (_directory, loader, project) = six_button_loader_and_project("tas-control-pce-six-button");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 107,
        profile: TasExecutionProfile::DirectPceSixButtonHuCard,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => {
            assert_eq!(
                witness.profile,
                TasExecutionProfile::DirectPceSixButtonHuCard
            );
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let input = TasInputFrame {
        p1_buttons: 0x93,
        p1_dpad: 0x04,
        ..Default::default()
    };
    expected.set_input(input.p1_buttons, input.p1_dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());
    assert!(
        emu_loop.handle_command(EmuCommand::ExecuteTasControl(Box::new(
            TasExecutionRequest {
                profile: TasExecutionProfile::DirectPceSixButtonHuCard,
                lease_id,
                run_id: 1,
                intermediate_cache_proofs: Vec::new(),
                cache_proof: TasExecutionCacheProof {
                    sync_identity_sha256: TasDigest([0x51; 32]),
                    branch_prefix_sha256: TasDigest([0x61; 32]),
                    target_cursor: 1,
                },
                predecessor_window: None,
                start_state_bytes: start_state,
                input_prefix: vec![input],
            },
        )))
    );
    match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectPceSixButtonHuCard,
            ..
        } => {}
        EmuResponse::TasExecutionRejected { reason, .. } => {
            panic!("six-button execution rejected: {reason:?}")
        }
        _ => panic!("unexpected execution response"),
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

#[test]
fn direct_pce_cd_chd_memory_base_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _memory_base_catalog) =
        chd_loader_and_project("tas-control-direct-pce-cd-chd");
    execute_direct_pce_cd_memory_base_worker(loader, project);
}

#[test]
fn direct_pce_cd_iso_memory_base_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _memory_base_catalog) =
        iso_loader_and_project("tas-control-direct-pce-cd-iso");
    execute_direct_pce_cd_memory_base_worker(loader, project);
}

#[test]
fn direct_pce_cd_ppf_memory_base_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _memory_base_catalog) =
        ppf_loader_and_project("tas-control-direct-pce-cd-ppf");
    execute_direct_pce_cd_memory_base_worker(loader, project);
}

fn execute_direct_pce_cd_memory_base_worker(
    loader: DirectPceCdTasExecutionLoader,
    project: crate::tas_project::TasProject,
) {
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    assert_eq!(
        backend.pce().unwrap().memory_base_mode(),
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled
    );
    let checkpoint = backend.encode_state_bytes().unwrap();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 108,
        profile: TasExecutionProfile::DirectPceCd,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => {
            assert_eq!(witness.profile, TasExecutionProfile::DirectPceCd);
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let input = TasInputFrame {
        p1_buttons: 0x03,
        p1_dpad: 0x04,
        ..Default::default()
    };
    expected.set_input(input.p1_buttons, input.p1_dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());
    let cache_proof = TasExecutionCacheProof {
        sync_identity_sha256: TasDigest([0x52; 32]),
        branch_prefix_sha256: TasDigest([0x62; 32]),
        target_cursor: 1,
    };
    assert!(
        emu_loop.handle_command(EmuCommand::ExecuteTasControl(Box::new(
            TasExecutionRequest {
                profile: TasExecutionProfile::DirectPceCd,
                lease_id,
                run_id: 1,
                intermediate_cache_proofs: Vec::new(),
                cache_proof,
                predecessor_window: None,
                start_state_bytes: start_state,
                input_prefix: vec![input],
            },
        )))
    );
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectPceCd,
            frame_count,
            state_sha256,
            ..
        } => (frame_count, state_sha256),
        _ => panic!("unexpected execution response"),
    };
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );

    let next_input = TasInputFrame {
        p1_buttons: 0x08,
        p1_dpad: 0x01,
        ..Default::default()
    };
    expected.set_input(next_input.p1_buttons, next_input.p1_dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectPceCd,
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
            profile: TasExecutionProfile::DirectPceCd,
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
        EmuResponse::TasControlRolledBack { .. }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

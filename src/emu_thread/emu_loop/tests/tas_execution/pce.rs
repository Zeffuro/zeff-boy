use super::super::support::tas_nes_test_loop_from_backend;
use crate::emu_backend::loader::{DirectPceCdTasExecutionLoader, DirectPceTasExecutionLoader};
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionCacheProof, TasExecutionProfile,
    TasExecutionRejectedReason, TasExecutionRequest, TasFrameAdvanceRequest, TasInputFrame,
};
use crate::tas_project::TasDigest;

#[path = "pce/arcade_multitap.rs"]
mod arcade_multitap;
mod memory_base_multitap;
#[path = "pce/ppf_multitap.rs"]
mod ppf_multitap;

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

fn multitap_loader_and_project(
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
    let loader = DirectPceTasExecutionLoader::new_multitap(path);
    let project = loader.create_project().unwrap();
    (directory, loader, project)
}

fn cd_multitap_loader_and_project(
    label: &str,
    chd: bool,
) -> (
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::tas_project::TasProject,
    crate::emu_backend::pce_profiles::TestControllerCatalogGuard,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let source_path = if chd {
        let path = directory.path().join("disc.chd");
        crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&path).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[4 * 2_448] ^= label.bytes().fold(0, u8::wrapping_add);
        std::fs::write(&path, bytes).unwrap();
        path
    } else {
        let path = directory.path().join("disc.cue");
        std::fs::write(
            directory.path().join("disc.bin"),
            vec![0xB7; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
        )
        .unwrap();
        std::fs::write(
            &path,
            b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
        )
        .unwrap();
        path
    };
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
    let catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        zeff_pce_core::hardware::PceControllerMode::Multitap,
    );
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        source_path,
        system_card,
        firmware_sha256,
    );
    let project = loader.create_project().unwrap();
    (directory, loader, project, catalog)
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
    let mut bytes = std::fs::read(&chd_path).unwrap();
    bytes[4 * 2_448] ^= label.bytes().fold(0, u8::wrapping_add);
    std::fs::write(&chd_path, bytes).unwrap();
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
    std::fs::write(directory.path().join("disc.bin"), vec![0xD3; 4 * 2048]).unwrap();
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

fn ppf_arcade_loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::tas_project::TasProject,
    crate::emu_backend::pce_profiles::TestArcadeCardCatalogGuard,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let cue_path = directory.path().join("disc.cue");
    std::fs::write(directory.path().join("disc.bin"), vec![0xD4; 4 * 2048]).unwrap();
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
    let arcade_catalog = crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
        source_disc_sha256,
    );
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![("arcade.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )
    .unwrap();
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path,
        system_card,
        system_card_sha256,
        stack,
    );
    let project = loader.create_project().unwrap();
    (directory, loader, project, arcade_catalog)
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
fn direct_pce_multitap_worker_dispatches_five_ports_and_restores_checkpoint() {
    let (_directory, loader, project) = multitap_loader_and_project("tas-control-pce-multitap");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 109,
        profile: TasExecutionProfile::DirectPceMultitapHuCard,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => {
            assert_eq!(
                witness.profile,
                TasExecutionProfile::DirectPceMultitapHuCard
            );
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let input = TasInputFrame {
        p1_buttons: 0x01,
        p1_dpad: 0x02,
        p2_buttons: 0x03,
        p2_dpad: 0x04,
        p3_buttons: 0x05,
        p3_dpad: 0x06,
        p4_buttons: 0x07,
        p4_dpad: 0x08,
        p5_buttons: 0x09,
        p5_dpad: 0x0A,
        ..Default::default()
    };
    expected.set_input(input.p1_buttons, input.p1_dpad);
    expected.set_input_p2(input.p2_buttons, input.p2_dpad);
    expected.set_input_p3(input.p3_buttons, input.p3_dpad);
    expected.set_input_p4(input.p4_buttons, input.p4_dpad);
    expected.set_input_p5(input.p5_buttons, input.p5_dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());
    let cache_proof = TasExecutionCacheProof {
        sync_identity_sha256: TasDigest([0x53; 32]),
        branch_prefix_sha256: TasDigest([0x63; 32]),
        target_cursor: 1,
    };
    assert!(
        emu_loop.handle_command(EmuCommand::ExecuteTasControl(Box::new(
            TasExecutionRequest {
                profile: TasExecutionProfile::DirectPceMultitapHuCard,
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
            profile: TasExecutionProfile::DirectPceMultitapHuCard,
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
        p1_buttons: 0x0B,
        p1_dpad: 0x0C,
        p2_buttons: 0x0D,
        p2_dpad: 0x0E,
        p3_buttons: 0x0F,
        p3_dpad: 0x01,
        p4_buttons: 0x02,
        p4_dpad: 0x03,
        p5_buttons: 0x04,
        p5_dpad: 0x05,
        ..Default::default()
    };
    expected.set_input(next_input.p1_buttons, next_input.p1_dpad);
    expected.set_input_p2(next_input.p2_buttons, next_input.p2_dpad);
    expected.set_input_p3(next_input.p3_buttons, next_input.p3_dpad);
    expected.set_input_p4(next_input.p4_buttons, next_input.p4_dpad);
    expected.set_input_p5(next_input.p5_buttons, next_input.p5_dpad);
    expected.step_frame();
    expected.drain_audio_samples_into(&mut Vec::new());
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectPceMultitapHuCard,
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
            profile: TasExecutionProfile::DirectPceMultitapHuCard,
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
        request_id: 110,
        profile: TasExecutionProfile::DirectPceHuCard,
    }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 110,
            ..
        }
    ));
}

#[test]
fn direct_pce_cd_multitap_worker_dispatches_five_ports_and_restores_checkpoint() {
    for chd in [false, true] {
        let (_directory, loader, project, _catalog) = cd_multitap_loader_and_project(
            if chd {
                "tas-control-pce-cd-chd-multitap"
            } else {
                "tas-control-pce-cd-multitap"
            },
            chd,
        );
        let backend = loader.load_editor_engine(&project).unwrap().into_backend();
        let checkpoint = backend.encode_state_bytes().unwrap();
        let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
        assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
            request_id: 119,
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
        let input = TasInputFrame {
            p1_buttons: 1,
            p2_buttons: 2,
            p3_buttons: 4,
            p4_buttons: 8,
            p5_dpad: 1,
            ..Default::default()
        };
        assert!(
            emu_loop.handle_command(EmuCommand::ExecuteTasControl(Box::new(
                TasExecutionRequest {
                    profile: TasExecutionProfile::DirectPceMultitapCd,
                    lease_id,
                    run_id: 1,
                    intermediate_cache_proofs: Vec::new(),
                    cache_proof: TasExecutionCacheProof {
                        sync_identity_sha256: TasDigest([0x71; 32]),
                        branch_prefix_sha256: TasDigest([0x72; 32]),
                        target_cursor: 1,
                    },
                    predecessor_window: None,
                    start_state_bytes: start_state,
                    input_prefix: vec![input],
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
}

#[test]
fn direct_pce_cd_chd_memory_base_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _memory_base_catalog) =
        chd_loader_and_project("tas-control-direct-pce-cd-chd");
    execute_direct_pce_cd_worker(
        loader,
        project,
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
    );
}

#[test]
fn direct_pce_cd_iso_memory_base_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _memory_base_catalog) =
        iso_loader_and_project("tas-control-direct-pce-cd-iso");
    execute_direct_pce_cd_worker(
        loader,
        project,
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
    );
}

#[test]
fn direct_pce_cd_ppf_memory_base_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _memory_base_catalog) =
        ppf_loader_and_project("tas-control-direct-pce-cd-ppf");
    execute_direct_pce_cd_worker(
        loader,
        project,
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
    );
}

#[test]
fn direct_pce_cd_ppf_arcade_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _arcade_catalog) =
        ppf_arcade_loader_and_project("tas-control-direct-pce-cd-ppf-arcade");
    execute_direct_pce_cd_worker(
        loader,
        project,
        zeff_pce_core::hardware::PceMemoryBaseMode::Disabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Enabled,
    );
}

fn execute_direct_pce_cd_worker(
    loader: DirectPceCdTasExecutionLoader,
    project: crate::tas_project::TasProject,
    memory_base_mode: zeff_pce_core::hardware::PceMemoryBaseMode,
    arcade_card_mode: zeff_pce_core::hardware::PceArcadeCardMode,
) {
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    assert_eq!(backend.pce().unwrap().memory_base_mode(), memory_base_mode);
    assert_eq!(backend.pce().unwrap().arcade_card_mode(), arcade_card_mode);
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

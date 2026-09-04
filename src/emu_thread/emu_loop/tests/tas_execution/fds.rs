use super::super::support::tas_nes_test_loop_from_backend;
use crate::emu_backend::loader::{BackendLoadConfig, DirectFdsTasExecutionLoader};
use crate::emu_backend::{ActiveSystem, load_backend_from_rom_source};
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionCacheProof, TasExecutionPredecessorWindow,
    TasExecutionProfile, TasExecutionRequest, TasFdsMediaEvent, TasFrameAdvanceRequest,
    TasInputFrame,
};
use crate::tas_project::TasDigest;

static FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
    [0xEA; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];

fn project_and_backend() -> (
    crate::tas_project::TasProject,
    crate::emu_backend::EmuBackend,
) {
    let directory = crate::test_support::test_directory("tas-worker-fds").unwrap();
    let path = directory.path().join("game.fds");
    let disk = (0..5 * zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE)
        .map(|index| index as u8)
        .collect::<Vec<_>>();
    std::fs::write(&path, disk).unwrap();
    let project = DirectFdsTasExecutionLoader::new_with_bios_override(path.clone(), &FDS_BIOS)
        .create_project()
        .unwrap();
    let backend = DirectFdsTasExecutionLoader::new_for_project(path, Vec::new(), &project)
        .unwrap()
        .with_project_bios_override(&FDS_BIOS)
        .load_fresh_backend()
        .unwrap();
    (project, backend)
}

fn request(
    lease_id: u64,
    run_id: u64,
    start_state_bytes: Vec<u8>,
    input_prefix: Vec<TasInputFrame>,
) -> EmuCommand {
    EmuCommand::ExecuteTasControl(Box::new(TasExecutionRequest {
        profile: TasExecutionProfile::DirectFdsDisk,
        lease_id,
        run_id,
        cache_proof: TasExecutionCacheProof {
            sync_identity_sha256: TasDigest([0x31; 32]),
            branch_prefix_sha256: TasDigest([0x32; 32]),
            target_cursor: input_prefix.len() as u64,
        },
        intermediate_cache_proofs: Vec::new(),
        predecessor_window: None,
        start_state_bytes,
        input_prefix,
    }))
}

#[test]
fn linked_fds_event_advance_and_rollback_restore_owned_state() {
    let (project, backend) = project_and_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 901,
        profile: TasExecutionProfile::DirectFdsDisk,
    }));
    let lease_id = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => {
            assert_eq!(witness.profile, TasExecutionProfile::DirectFdsDisk);
            lease_id
        }
        _ => panic!("unexpected acquisition response"),
    };
    let side_input = TasInputFrame {
        fds_disk_side: Some(4),
        ..Default::default()
    };
    let first = request(
        lease_id,
        1,
        project.start_state().to_vec(),
        vec![side_input],
    );
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
    let suffix = vec![
        TasInputFrame {
            fds_write_protected: Some(true),
            ..Default::default()
        },
        TasInputFrame {
            fds_media_event: Some(TasFdsMediaEvent::Eject),
            ..Default::default()
        },
    ];
    let mut second = request(
        lease_id,
        2,
        project.start_state().to_vec(),
        std::iter::once(side_input)
            .chain(suffix.iter().copied())
            .collect(),
    );
    let EmuCommand::ExecuteTasControl(second_request) = &mut second else {
        unreachable!()
    };
    second_request.predecessor_window = Some(TasExecutionPredecessorWindow {
        source_proofs: vec![second_request.cache_proof, first_proof],
        input_start_cursor: 1,
        input_frames: suffix,
    });
    assert!(emu_loop.handle_command(second));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            frame_count,
            state_sha256,
            segment_frame_count: 2,
            executed_project_frames: 3,
            ..
        } => (frame_count, state_sha256),
        _ => panic!("unexpected execution response"),
    };
    assert!(
        !emu_loop
            .backend
            .nes()
            .unwrap()
            .media_slot_snapshot()
            .unwrap()
            .inserted()
    );
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectFdsDisk,
                lease_id,
                run_id: 2,
                advance_id: 1,
                segment_id: 1,
                expected_segment_frame_count: 2,
                expected_executed_project_frames: 3,
                expected_frame_count: frame_count,
                expected_state_sha256: state_sha256,
                input: TasInputFrame {
                    p1_buttons: 1,
                    fds_media_event: Some(TasFdsMediaEvent::Insert {
                        side: 4,
                        write_protected: false,
                    }),
                    ..Default::default()
                },
                snapshot: None,
            },
        )))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanced { .. }
    ));
    assert_eq!(emu_loop.backend.nes().unwrap().fds_disk_side(), Some(4));
    assert!(
        !emu_loop
            .backend
            .nes()
            .unwrap()
            .media_slot_snapshot()
            .unwrap()
            .state
            .write_protected
    );
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { .. }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

#[test]
fn ordinary_host_persistent_fds_backend_is_not_acquirable() {
    let directory = crate::test_support::test_directory("tas-worker-fds-in-place").unwrap();
    let path = directory.path().join("game.fds");
    let disk = (0..zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE)
        .map(|index| index as u8)
        .collect::<Vec<_>>();
    std::fs::write(&path, &disk).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &path,
        &path,
        Some(disk),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            apply_mods: false,
            initial_input: None,
            nes_load_battery_sram: false,
            fds_bios_override: Some(&FDS_BIOS),
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    assert!(backend.nes().unwrap().host_persistence_enabled());
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 902,
        profile: TasExecutionProfile::DirectFdsDisk,
    }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 902,
            ..
        }
    ));
}

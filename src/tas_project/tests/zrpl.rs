use zeff_emu_common::replay::{
    ReplayCheckpoint, ReplayColecoControllerFrame, ReplayEvent, ReplayFirmwareManifest,
    ReplayJoypadFrame, ReplayMetadata, ReplayPlayer, ReplayRecorder, ReplayZapperFrame,
};

use super::super::*;
use super::{project, zrpl_test_dir};

#[test]
fn zrpl_import_export_roundtrips_every_input_channel_and_verification() {
    let directory = zrpl_test_dir("roundtrip");
    let source_path = directory.join("source.zrpl");
    let output_path = directory.join("output.zrpl");
    let start_state = vec![0xC3; 257];
    let camera = vec![0x10, 0x20, 0x30, 0x40];
    let input = ReplayJoypadFrame {
        buttons: 1,
        dpad: 2,
        buttons_p2: 3,
        dpad_p2: 4,
        buttons_p3: 5,
        dpad_p3: 6,
        buttons_p4: 7,
        dpad_p4: 8,
        buttons_p5: 9,
        dpad_p5: 10,
        zapper: ReplayZapperFrame {
            enabled: true,
            trigger: true,
            hit: false,
            screen_pos: Some((0, 0)),
        },
        host_tilt: (f32::from_bits(0x7FC0_1234), f32::from_bits(0x8000_0000)),
        camera_frame: Some(camera.clone()),
        coleco: Default::default(),
    };
    let metadata = replay_metadata(Some([0x99; 32]));
    let mut recorder = ReplayRecorder::new_with_metadata(
        source_path.clone(),
        start_state.clone(),
        metadata.clone(),
    );
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    recorder.record_joypad_frame(input.clone());
    recorder.record_joypad_frame(input.clone());
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    recorder.record_joypad_frame(input.clone());
    recorder.finish().unwrap();
    let source_bytes = std::fs::read(&source_path).unwrap();

    let project =
        TasProject::import_zrpl(&source_path, replay_witness(&start_state, Some([0x99; 32])))
            .unwrap();
    assert_eq!(
        project.source_replay_sha256,
        Some(TasDigest::from_bytes(&source_bytes))
    );
    let persisted = TasProject::decode(&project.encode().unwrap()).unwrap();
    assert_eq!(persisted.source_replay_sha256, project.source_replay_sha256);
    assert_eq!(project.branches[0].frame_count, 5);
    assert_eq!(project.branches[0].input_spans.len(), 2);
    assert_eq!(project.branches[0].input_spans[0].length, 2);
    assert_eq!(project.assets.len(), 1);
    assert_eq!(project.replay_start.wonder_swan_link_tick, Some(456));
    assert!(project.verification_is_current("main").unwrap());

    let without_provenance = TasProject {
        source_replay_sha256: None,
        ..project.clone()
    };
    assert_eq!(
        project.sync_identity_sha256().unwrap(),
        without_provenance.sync_identity_sha256().unwrap()
    );
    assert_eq!(
        project.branch_movie_sha256("main").unwrap(),
        without_provenance.branch_movie_sha256("main").unwrap()
    );

    project
        .export_zrpl_without_execution_for_test("main", &output_path)
        .unwrap();
    let source = ReplayPlayer::load(&source_path).unwrap();
    let output = ReplayPlayer::load(&output_path).unwrap();
    assert_eq!(output.save_state(), source.save_state());
    assert_eq!(output.metadata(), source.metadata());
    assert_eq!(
        output.peek_joypad_frames(0, output.total_frames()),
        source.peek_joypad_frames(0, source.total_frames())
    );
    assert!(
        project
            .export_zrpl_without_execution_for_test("main", &output_path)
            .is_err()
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn zrpl_import_rejects_legacy_and_every_mismatched_witness_domain() {
    let directory = zrpl_test_dir("witness");
    let source_path = directory.join("source.zrpl");
    let start_state = vec![0x5A; 64];
    write_replay_fixture(&source_path, &start_state, Some([0x99; 32]));

    let mut witnesses = Vec::new();
    let mut system = replay_witness(&start_state, Some([0x99; 32]));
    system.identity.system = "gb".to_owned();
    witnesses.push(system);
    let mut core = replay_witness(&start_state, Some([0x99; 32]));
    core.identity.core_family = "GameBoy".to_owned();
    witnesses.push(core);
    let mut media = replay_witness(&start_state, Some([0x99; 32]));
    media.identity.effective_media_sha256.0[0] ^= 1;
    witnesses.push(media);
    let mut firmware = replay_witness(&start_state, Some([0x99; 32]));
    firmware.identity.firmware.clear();
    witnesses.push(firmware);
    let cheats = replay_witness(&start_state, None);
    witnesses.push(cheats);
    let mut state = replay_witness(&start_state, Some([0x99; 32]));
    state.identity.start_state_sha256.0[0] ^= 1;
    witnesses.push(state);

    for witness in witnesses {
        assert!(TasProject::import_zrpl(&source_path, witness).is_err());
    }

    let legacy_path = directory.join("legacy.zrpl");
    let mut legacy = std::fs::read(&source_path).unwrap();
    legacy[4..8].copy_from_slice(&1u32.to_le_bytes());
    std::fs::write(&legacy_path, legacy).unwrap();
    assert!(
        TasProject::import_zrpl(&legacy_path, replay_witness(&start_state, Some([0x99; 32])))
            .is_err()
    );

    let old_metadata_path = directory.join("old-metadata.zrpl");
    let mut old_metadata = std::fs::read(&source_path).unwrap();
    old_metadata[12..16].copy_from_slice(&2u32.to_le_bytes());
    std::fs::write(&old_metadata_path, old_metadata).unwrap();
    assert!(
        TasProject::import_zrpl(
            &old_metadata_path,
            replay_witness(&start_state, Some([0x99; 32]))
        )
        .is_err()
    );

    let empty_metadata_path = directory.join("empty-metadata.zrpl");
    ReplayRecorder::new(empty_metadata_path.clone(), start_state.clone())
        .finish()
        .unwrap();
    assert!(
        TasProject::import_zrpl(
            &empty_metadata_path,
            replay_witness(&start_state, Some([0x99; 32]))
        )
        .is_err()
    );

    let duplicate_checkpoint_path = directory.join("duplicate-checkpoint.zrpl");
    let mut duplicate_metadata = replay_metadata(Some([0x99; 32]));
    duplicate_metadata.checkpoints = vec![
        ReplayCheckpoint {
            frame: 0,
            state_sha256: [1; 32],
        },
        ReplayCheckpoint {
            frame: 0,
            state_sha256: [2; 32],
        },
    ];
    ReplayRecorder::new_with_metadata(
        duplicate_checkpoint_path.clone(),
        start_state.clone(),
        duplicate_metadata,
    )
    .finish()
    .unwrap();
    assert!(
        TasProject::import_zrpl(
            &duplicate_checkpoint_path,
            replay_witness(&start_state, Some([0x99; 32]))
        )
        .is_err()
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn zrpl_export_rejects_stale_verification_and_empty_camera_assets() {
    let directory = zrpl_test_dir("export-gates");
    let source_path = directory.join("source.zrpl");
    let start_state = vec![0x6B; 64];
    write_replay_fixture(&source_path, &start_state, None);
    let project =
        TasProject::import_zrpl(&source_path, replay_witness(&start_state, None)).unwrap();

    let mut stale = project.clone();
    stale.branches[0].input_spans[0].input.players[0].buttons ^= 2;
    let stale_path = directory.join("stale.zrpl");
    assert!(
        stale
            .export_zrpl_without_execution_for_test("main", &stale_path)
            .is_err()
    );
    assert!(!stale_path.exists());
    assert!(
        stale
            .export_zrpl_without_execution_for_test("missing", &stale_path)
            .is_err()
    );

    let mut empty_camera = project;
    let empty_digest = TasDigest::from_bytes(&[]);
    empty_camera.assets.clear();
    empty_camera.assets.insert(empty_digest, Vec::new());
    empty_camera.branches[0].input_spans[0].input.camera = TasCameraInput::Blob(empty_digest);
    empty_camera.branches[0].verification = None;
    let empty_path = directory.join("empty.zrpl");
    assert!(empty_camera.validate().is_ok());
    assert!(
        empty_camera
            .export_zrpl_without_execution_for_test("main", &empty_path)
            .is_err()
    );
    assert!(!empty_path.exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn zrpl_conversion_roundtrips_exact_coleco_controller_topology() {
    let directory = zrpl_test_dir("coleco-controller-topology");
    let source_path = directory.join("source.zrpl");
    let output_path = directory.join("output.zrpl");
    let start_state = vec![0x6C; 64];
    let mut witness = replay_witness(&start_state, None);
    witness.identity.system = "coleco".to_owned();
    witness.identity.core_family = "ColecoVision".to_owned();
    witness.identity.firmware.clear();
    witness.identity.devices = (1..=2)
        .map(|port| TasDeviceIdentity {
            port: format!("p{port}"),
            device: "coleco-standard-controller-keypad".to_owned(),
            configuration_sha256: TasDigest([0xC0; 32]),
        })
        .collect();
    let metadata = ReplayMetadata {
        system: Some(witness.identity.system.clone()),
        core_family: Some(witness.identity.core_family.clone()),
        rom_sha256: Some(witness.identity.effective_media_sha256.0),
        ..ReplayMetadata::default()
    };
    let input = ReplayJoypadFrame {
        coleco: [
            ReplayColecoControllerFrame {
                down: true,
                left_button: true,
                keypad: 7,
                ..ReplayColecoControllerFrame::default()
            },
            ReplayColecoControllerFrame {
                right: true,
                right_button: true,
                keypad: 12,
                ..ReplayColecoControllerFrame::default()
            },
        ],
        ..ReplayJoypadFrame::default()
    };
    let mut recorder =
        ReplayRecorder::new_with_metadata(source_path.clone(), start_state, metadata);
    recorder.enable_coleco_input_format();
    recorder.record_joypad_frame(input.clone());
    recorder.finish().unwrap();

    let project = TasProject::import_zrpl(&source_path, witness.clone()).unwrap();
    assert_eq!(
        project.branches[0].input_spans[0].input.coleco[0].keypad,
        TasColecoKeypadKey::Six
    );
    assert_eq!(
        project.branches[0].input_spans[0].input.coleco[1].keypad,
        TasColecoKeypadKey::Pound
    );
    project
        .export_zrpl_without_execution_for_test("main", &output_path)
        .unwrap();
    let mut player = ReplayPlayer::load(&output_path).unwrap();
    assert_eq!(
        u32::from_le_bytes(
            std::fs::read(&output_path).unwrap()[4..8]
                .try_into()
                .unwrap()
        ),
        3
    );
    assert_eq!(player.next_joypad_frame(), Some(input));

    let mut wrong_topology = witness;
    wrong_topology.identity.devices.pop();
    let error = TasProject::import_zrpl(&source_path, wrong_topology).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires two standard controller/keypad devices")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

fn replay_witness(start_state: &[u8], cheat_sha256: Option<[u8; 32]>) -> TasZrplImportWitness {
    let mut identity = project().identity;
    identity.start_state_sha256 = TasDigest::from_bytes(start_state);
    identity.firmware = vec![
        TasFirmwareIdentity::Skipped {
            firmware_id: "d-skipped".to_owned(),
            compatibility_version: 4,
        },
        TasFirmwareIdentity::BuiltinOpenSource {
            firmware_id: "c-open".to_owned(),
            implementation: "open-v1".to_owned(),
            compatibility_version: 3,
            sha256: TasDigest([3; 32]),
        },
        TasFirmwareIdentity::Hle {
            firmware_id: "b-hle".to_owned(),
            implementation: "hle-v1".to_owned(),
            compatibility_version: 2,
        },
        TasFirmwareIdentity::External {
            firmware_id: "a-external".to_owned(),
            variant: Some("variant-a".to_owned()),
            sha256: TasDigest([1; 32]),
        },
    ];
    identity.cheats = cheat_sha256.map_or(TasExternalIdentity::Absent, |sha256| {
        TasExternalIdentity::ExternalSha256(TasDigest(sha256))
    });
    TasZrplImportWitness {
        project_id: "imported-project".to_owned(),
        identity,
    }
}

fn replay_metadata(cheat_sha256: Option<[u8; 32]>) -> ReplayMetadata {
    ReplayMetadata {
        system: Some("nes".to_owned()),
        core_family: Some("zeff-nes".to_owned()),
        rom_sha256: Some([0x22; 32]),
        firmware: vec![
            ReplayFirmwareManifest::External {
                firmware_id: "a-external".to_owned(),
                variant: Some("variant-a".to_owned()),
                sha256: [1; 32],
            },
            ReplayFirmwareManifest::Hle {
                firmware_id: "b-hle".to_owned(),
                implementation: "hle-v1".to_owned(),
                compatibility_version: 2,
            },
            ReplayFirmwareManifest::BuiltinOpenSource {
                firmware_id: "c-open".to_owned(),
                implementation: "open-v1".to_owned(),
                compatibility_version: 3,
                sha256: [3; 32],
            },
            ReplayFirmwareManifest::Skipped {
                firmware_id: "d-skipped".to_owned(),
                compatibility_version: 4,
            },
        ],
        events: vec![ReplayEvent::FdsDiskSide { frame: 1, side: 1 }],
        cheat_sha256,
        final_state_sha256: Some([0xAB; 32]),
        wonder_swan_link_start_tick: Some(456),
        checkpoints: vec![
            ReplayCheckpoint {
                frame: 0,
                state_sha256: [0xCD; 32],
            },
            ReplayCheckpoint {
                frame: 5,
                state_sha256: [0xEF; 32],
            },
        ],
        ..ReplayMetadata::default()
    }
}

fn write_replay_fixture(path: &std::path::Path, start_state: &[u8], cheat: Option<[u8; 32]>) {
    let mut recorder = ReplayRecorder::new_with_metadata(
        path.to_path_buf(),
        start_state.to_vec(),
        replay_metadata(cheat),
    );
    recorder.record_joypad_frame(ReplayJoypadFrame {
        buttons: 1,
        camera_frame: Some(vec![1, 2, 3]),
        ..ReplayJoypadFrame::default()
    });
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    recorder.finish().unwrap();
}

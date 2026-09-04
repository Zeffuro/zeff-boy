use std::collections::BTreeMap;

use zeff_emu_common::replay::{ReplayPlayer, ReplayStartMetadata};

use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};

use super::super::verification::TasExecutionSession;
use super::super::*;
use super::zrpl_test_dir;

#[test]
fn emulator_verification_is_two_pass_deterministic_and_exports_embedded_provenance()
-> anyhow::Result<()> {
    let directory = zrpl_test_dir("executed-verification");
    let rom_path = directory.join("test.nes");
    let (mut project, witness, rom) = executable_nes_project(&rom_path, 601)?;
    let edit_generation = project.edit_generation;
    let rerecord_count = project.rerecord_count;
    let mut loads = 0;

    let verification = project.verify_branch_with_factory("main", &witness, || {
        loads += 1;
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    })?;

    assert_eq!(loads, 2);
    assert_eq!(
        verification
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.cursor)
            .collect::<Vec<_>>(),
        vec![300, 600]
    );
    assert!(verification.final_state_sha256.is_some());
    assert_eq!(project.edit_generation, edit_generation);
    assert_eq!(project.rerecord_count, rerecord_count);
    assert!(project.verification_is_current("main")?);

    let mut repeat_loads = 0;
    let repeated = project.verify_branch_with_factory("main", &witness, || {
        repeat_loads += 1;
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    })?;
    assert_eq!(repeat_loads, 2);
    assert_eq!(repeated, verification);

    let output_path = directory.join("verified.zrpl");
    let mut export_loads = 0;
    project.verify_and_export_zrpl_with_factory("main", &output_path, &witness, || {
        export_loads += 1;
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    })?;
    assert_eq!(export_loads, 2);
    let output = ReplayPlayer::load(&output_path)?;
    assert_eq!(output.total_frames(), 601);
    assert_eq!(output.metadata().checkpoints.len(), 2);
    assert_eq!(
        output.metadata().final_state_sha256,
        verification.final_state_sha256.map(|digest| digest.0)
    );

    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn emulator_verification_failures_are_transactional_and_require_complete_identity()
-> anyhow::Result<()> {
    let directory = zrpl_test_dir("executed-verification-failures");
    let rom_path = directory.join("test.nes");
    let (project, witness, rom) = executable_nes_project(&rom_path, 301)?;

    let mut witness_mismatches = Vec::new();
    let mut identity = witness.identity.clone();
    identity.system.push('2');
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.core_family.push('2');
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.determinism_abi.push('2');
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.source_media_sha256.0[0] ^= 1;
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.effective_media_sha256.0[0] ^= 1;
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.patches.push(TasPatchIdentity {
        format: "ips".to_owned(),
        sha256: TasDigest([1; 32]),
    });
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.firmware.push(TasFirmwareIdentity::Skipped {
        firmware_id: "test-firmware".to_owned(),
        compatibility_version: 1,
    });
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.devices[0].configuration_sha256.0[0] ^= 1;
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.sync_config_sha256.0[0] ^= 1;
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.persistent_state = TasExternalIdentity::ExternalSha256(TasDigest([2; 32]));
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.rtc_state = TasExternalIdentity::ExternalSha256(TasDigest([3; 32]));
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.sensor_state = TasExternalIdentity::ExternalSha256(TasDigest([4; 32]));
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.cheats = TasExternalIdentity::ExternalSha256(TasDigest([5; 32]));
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.state_format_compatibility_id.push('2');
    witness_mismatches.push(identity);
    let mut identity = witness.identity.clone();
    identity.start_state_sha256.0[0] ^= 1;
    witness_mismatches.push(identity);

    for identity in witness_mismatches {
        let mismatched_witness = TasExecutionWitness { identity };
        let mut witness_failure = project.clone();
        let before = witness_failure.encode()?;
        let result = witness_failure.verify_branch_with_factory(
            "main",
            &mismatched_witness,
            || -> anyhow::Result<TasExecutionSession> {
                panic!("backend must not load for witness mismatch")
            },
        );
        assert!(result.is_err());
        assert_eq!(witness_failure.encode()?, before);
    }

    let mut second_pass_failure = project.clone();
    let before = second_pass_failure.encode()?;
    let mut loads = 0;
    let result = second_pass_failure.verify_branch_with_factory("main", &witness, || {
        loads += 1;
        if loads == 2 {
            anyhow::bail!("injected second-pass backend failure");
        }
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    });
    assert!(result.is_err());
    assert_eq!(loads, 2);
    assert_eq!(second_pass_failure.encode()?, before);

    let mut first_session_identity_failure = project.clone();
    let before = first_session_identity_failure.encode()?;
    let mut mismatched_session_identity = witness.identity.clone();
    mismatched_session_identity.sync_config_sha256.0[0] ^= 1;
    let result =
        first_session_identity_failure.verify_branch_with_factory("main", &witness, || {
            Ok(TasExecutionSession::new(
                load_executable_nes_backend(&rom_path, rom.clone())?,
                mismatched_session_identity.clone(),
            ))
        });
    assert!(result.is_err());
    assert_eq!(first_session_identity_failure.encode()?, before);

    let mut second_session_identity_failure = project.clone();
    let before = second_session_identity_failure.encode()?;
    let mut loads = 0;
    let result =
        second_session_identity_failure.verify_branch_with_factory("main", &witness, || {
            loads += 1;
            let identity = if loads == 2 {
                mismatched_session_identity.clone()
            } else {
                witness.identity.clone()
            };
            Ok(TasExecutionSession::new(
                load_executable_nes_backend(&rom_path, rom.clone())?,
                identity,
            ))
        });
    assert!(result.is_err());
    assert_eq!(loads, 2);
    assert_eq!(second_session_identity_failure.encode()?, before);

    let failed_export_path = directory.join("failed-export.zrpl");
    let mut export_failure = project.clone();
    let before = export_failure.encode()?;
    let mut export_loads = 0;
    let result = export_failure.verify_and_export_zrpl_with_factory(
        "main",
        &failed_export_path,
        &witness,
        || {
            export_loads += 1;
            if export_loads == 2 {
                anyhow::bail!("injected temporary replay execution failure");
            }
            load_executable_nes_session(&rom_path, rom.clone(), &witness)
        },
    );
    assert!(result.is_err());
    assert_eq!(export_loads, 2);
    assert!(!failed_export_path.exists());
    assert!(
        std::fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp."))
    );
    assert_eq!(export_failure.encode()?, before);

    let existing_path = directory.join("existing.zrpl");
    std::fs::write(&existing_path, b"concurrent replay")?;
    let mut existing_failure = project.clone();
    let before = existing_failure.encode()?;
    let result = existing_failure.verify_and_export_zrpl_with_factory(
        "main",
        &existing_path,
        &witness,
        || -> anyhow::Result<TasExecutionSession> {
            panic!("backend must not load when export target already exists")
        },
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read(&existing_path)?, b"concurrent replay");
    assert_eq!(existing_failure.encode()?, before);

    let mut wrong_backend = project.clone();
    let before = wrong_backend.encode()?;
    let mut wrong_rom = rom.clone();
    wrong_rom[16] ^= 1;
    let result = wrong_backend.verify_branch_with_factory("main", &witness, || {
        load_executable_nes_session(&rom_path, wrong_rom.clone(), &witness)
    });
    assert!(result.is_err());
    assert_eq!(wrong_backend.encode()?, before);

    let mut cheat_project = project.clone();
    cheat_project.identity.cheats = TasExternalIdentity::ExternalSha256(TasDigest([0xA5; 32]));
    let cheat_witness = TasExecutionWitness {
        identity: cheat_project.identity.clone(),
    };
    let before = cheat_project.encode()?;
    let result = cheat_project.verify_branch_with_factory("main", &cheat_witness, || {
        load_executable_nes_session(&rom_path, rom.clone(), &cheat_witness)
    });
    assert!(result.is_err());
    assert_eq!(cheat_project.encode()?, before);

    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn emulator_verification_preserves_valid_imported_schedule_and_rejects_bad_hashes()
-> anyhow::Result<()> {
    let directory = zrpl_test_dir("executed-verification-existing");
    let rom_path = directory.join("test.nes");
    let (mut project, witness, rom) = executable_nes_project(&rom_path, 301)?;
    let generated = project.verify_branch_with_factory("main", &witness, || {
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    })?;

    let mut bad = project.clone();
    bad.branches[0].verification.as_mut().unwrap().checkpoints[0]
        .state_sha256
        .0[0] ^= 1;
    let before = bad.encode()?;
    let result = bad.verify_branch_with_factory("main", &witness, || {
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    });
    assert!(result.is_err());
    assert_eq!(bad.encode()?, before);

    let mut stale = project.clone();
    stale.branches[0].input_spans[0].input.players[0].buttons ^= 2;
    assert!(!stale.verification_is_current("main")?);
    let refreshed = stale.verify_branch_with_factory("main", &witness, || {
        load_executable_nes_session(&rom_path, rom.clone(), &witness)
    })?;
    assert_ne!(refreshed.branch_movie_sha256, generated.branch_movie_sha256);
    assert_eq!(
        refreshed
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.cursor)
            .collect::<Vec<_>>(),
        vec![300]
    );
    assert!(stale.verification_is_current("main")?);

    std::fs::remove_dir_all(directory)?;
    Ok(())
}

fn build_executable_nes_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let prg = 16;
    rom[prg] = 0xA9;
    rom[prg + 1] = 0x42;
    rom[prg + 2] = 0x85;
    rom[prg + 3] = 0x00;
    rom[prg + 4] = 0x4C;
    rom[prg + 5] = 0x04;
    rom[prg + 6] = 0x80;
    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

fn load_executable_nes_backend(
    rom_path: &std::path::Path,
    rom: Vec<u8>,
) -> anyhow::Result<EmuBackend> {
    Ok(load_backend_from_rom_source(
        ActiveSystem::Nes,
        rom_path,
        rom_path,
        Some(rom),
        BackendLoadConfig::default(),
    )?
    .backend)
}

fn load_executable_nes_session(
    rom_path: &std::path::Path,
    rom: Vec<u8>,
    witness: &TasExecutionWitness,
) -> anyhow::Result<TasExecutionSession> {
    Ok(TasExecutionSession::new(
        load_executable_nes_backend(rom_path, rom)?,
        witness.identity.clone(),
    ))
}

fn executable_nes_project(
    rom_path: &std::path::Path,
    frame_count: u64,
) -> anyhow::Result<(TasProject, TasExecutionWitness, Vec<u8>)> {
    let rom = build_executable_nes_rom();
    let backend = load_executable_nes_backend(rom_path, rom.clone())?;
    let start_state = backend.encode_state_bytes()?;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .ok_or_else(|| anyhow::anyhow!("NES fixture is missing its ROM hash"))?,
    );
    let identity = TasProjectIdentity {
        system: metadata.system.unwrap(),
        core_family: metadata.core_family.unwrap(),
        determinism_abi: "nes-test-determinism-v1".to_owned(),
        source_media_sha256: effective_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: Vec::new(),
        devices: vec![TasDeviceIdentity {
            port: "p1".to_owned(),
            device: "gamepad".to_owned(),
            configuration_sha256: TasDigest([0; 32]),
        }],
        sync_config_sha256: TasDigest([0; 32]),
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: "nes-test-state-v1".to_owned(),
        start_state_sha256: TasDigest::from_bytes(&start_state),
    };
    let witness = TasExecutionWitness {
        identity: identity.clone(),
    };
    let project = TasProject {
        project_id: "executable-nes".to_owned(),
        source_replay_sha256: None,
        identity,
        start_state: start_state.into(),
        replay_start: ReplayStartMetadata::default(),
        edit_generation: 7,
        rerecord_count: 3,
        active_branch_id: "main".to_owned(),
        project_comment: String::new(),
        branches: vec![TasBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            comment: String::new(),
            parent: None,
            frame_count,
            input_spans: vec![TasInputSpan {
                start: 0,
                length: frame_count,
                input: TasInputFrame {
                    players: [
                        TasControllerInput {
                            buttons: 1,
                            dpad: 0,
                        },
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                    ],
                    ..TasInputFrame::default()
                },
            }],
            events: Vec::new(),
            verification: None,
        }],
        markers: Vec::new(),
        annotations: Vec::new(),
        assets: BTreeMap::new(),
    };
    project.validate()?;
    Ok((project, witness, rom))
}

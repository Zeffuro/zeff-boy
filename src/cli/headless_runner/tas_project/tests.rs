use std::collections::BTreeMap;

use zeff_emu_common::replay::{ReplayPlayer, ReplayStartMetadata};

use crate::emu_backend::loader::{
    DirectFdsTasExecutionLoader, DirectGameGearTasExecutionLoader, DirectNesTasExecutionLoader,
    DirectSg1000TasExecutionLoader, DirectSmsTasExecutionLoader, MAX_NES_CARTRIDGE_BYTES,
    direct_nes_tas_identity, read_nes_cartridge_bounded,
};
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::tas_project::{
    TasAnnotation, TasCameraInput, TasControllerInput, TasInitialBranch, TasInputFrame,
    TasInputSpan, TasMarker, TasZapperInput,
};

use super::*;
use crate::test_support::{test_directory, write_zip};

fn executable_nes_rom() -> Vec<u8> {
    let mut rom = vec![0; 16 + 0x4000 + 0x2000];
    rom[..4].copy_from_slice(b"NES\x1A");
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

fn codemasters_sms_rom() -> Vec<u8> {
    let offset = zeff_sega8_core::hardware::constants::CODEMASTERS_HEADER_OFFSET;
    let mut rom = vec![0xFF; offset + 16];
    rom[offset] = 2;
    rom[offset + 1..offset + 6].copy_from_slice(&[0x31, 0x08, 0x93, 0x10, 0x59]);
    rom[offset + 6..offset + 8].copy_from_slice(&0x1234u16.to_le_bytes());
    rom[offset + 8..offset + 10].copy_from_slice(&0xEDCCu16.to_le_bytes());
    rom[offset + 10..offset + 16].fill(0);
    rom
}

fn direct_game_gear_rom() -> Vec<u8> {
    let mut rom = vec![0x00; 16 * 1024];
    let offset = 0x3FF0;
    rom[offset..offset + 8].copy_from_slice(b"TMR SEGA");
    rom[offset + 0x0A..offset + 0x0C].copy_from_slice(&0x1234u16.to_le_bytes());
    rom[offset + 0x0C] = 0x42;
    rom[offset + 0x0D] = 0x31;
    rom[offset + 0x0E] = 0xA5;
    rom[offset + 0x0F] = 0x6A;
    rom
}

static FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
    [0xEA; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];

fn fds_disk(sides: usize) -> Vec<u8> {
    (0..sides)
        .flat_map(|side| {
            (0..zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE)
                .map(move |index| (side as u8).wrapping_mul(0x51).wrapping_add(index as u8))
        })
        .collect()
}

fn project_for_rom(rom_path: &Path, rom: &[u8], frames: u64) -> Result<TasProject> {
    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        rom_path,
        rom_path,
        Some(rom.to_vec()),
        BackendLoadConfig::default(),
    )?
    .backend;
    let start_state = backend.encode_state_bytes()?;
    project_for_rom_with_start_state(rom, frames, &backend, start_state)
}

fn project_for_rom_with_start_state(
    rom: &[u8],
    frames: u64,
    backend: &EmuBackend,
    start_state: Vec<u8>,
) -> Result<TasProject> {
    let identity = direct_nes_tas_identity(backend, rom, &start_state)?;
    TasProject::new(
        "cli-verification",
        identity,
        start_state,
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: frames,
            input_spans: vec![TasInputSpan {
                start: 0,
                length: frames,
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
        },
        BTreeMap::new(),
    )
}

#[test]
fn native_cli_verifies_saves_then_exports_with_loader_owned_identity() -> Result<()> {
    let directory = test_directory("tas-cli")?;
    let rom_path = directory.path().join("game.nes");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    let rom = executable_nes_rom();
    std::fs::write(&rom_path, &rom)?;
    project_for_rom(&rom_path, &rom, 301)?.save_atomic(&project_path)?;

    run_tas_project_headless(
        &rom_path,
        Vec::new(),
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;

    let saved = TasProject::load(&project_path)?;
    assert!(saved.verification_is_current("main")?);
    let verification = saved.branches()[0]
        .verification()
        .expect("verification should be persisted");
    assert_eq!(
        verification
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.cursor)
            .collect::<Vec<_>>(),
        vec![300]
    );
    let replay = ReplayPlayer::load(&export_path)?;
    assert_eq!(replay.total_frames(), 301);
    assert_eq!(replay.metadata().checkpoints.len(), 1);
    assert_eq!(
        replay.metadata().final_state_sha256,
        verification.final_state_sha256.map(|digest| digest.0)
    );
    Ok(())
}

#[test]
fn native_cli_two_pass_verifies_project_owned_selected_zip_five_side_fds() -> Result<()> {
    let directory = test_directory("tas-cli-fds")?;
    let archive_path = directory.path().join("game.zip");
    let project_path = directory.path().join("movie.ztas");
    write_zip(&archive_path, &[("set/game.fds", &fds_disk(5))])?;
    let mut project = DirectFdsTasExecutionLoader::new_zip_with_bios_override(
        archive_path.clone(),
        Some(archive_path.join("set/game.fds")),
        &FDS_BIOS,
    )
    .create_project()?;
    project.edit_transaction(|edit| {
        edit.insert_frames("main", 1, 1)?;
        edit.replace_branch_events(
            "main",
            vec![zeff_emu_common::replay::ReplayEvent::FdsDiskSide { frame: 0, side: 4 }],
        )?;
        edit.set_input_range(
            "main",
            0,
            2,
            TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 0x01,
                        dpad: 0x04,
                    },
                    TasControllerInput {
                        buttons: 0x02,
                        dpad: 0x08,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        )
    })?;
    project.save_atomic(&project_path)?;
    write_zip(&archive_path, &[("changed.fds", &fds_disk(1))])?;
    let loader = DirectFdsTasExecutionLoader::new_for_project(archive_path, Vec::new(), &project)?
        .with_project_bios_override(&FDS_BIOS);
    run_tas_project_headless_with_plan(
        PrivateTasExecutionLoader::DirectFds(loader),
        &project_path,
        "main",
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;
    let saved = TasProject::load(&project_path)?;
    assert!(saved.verification_is_current("main")?);
    assert_eq!(
        saved.branches()[0]
            .verification()
            .unwrap()
            .checkpoints
            .len(),
        0
    );
    Ok(())
}

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_sms_two_pad_input() -> Result<()> {
    let directory = test_directory("tas-cli-sms")?;
    let rom_path = directory.path().join("game.sms");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    std::fs::write(&rom_path, codemasters_sms_rom())?;
    let loader = DirectSmsTasExecutionLoader::new(rom_path.clone());
    let mut project = loader.create_project()?;
    project.edit_transaction(|edit| {
        edit.insert_frames("main", 1, 1)?;
        edit.set_input_range(
            "main",
            0,
            2,
            TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 0x01,
                        dpad: 0x04,
                    },
                    TasControllerInput {
                        buttons: 0x02,
                        dpad: 0x08,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        )
    })?;
    project.save_atomic(&project_path)?;

    run_tas_project_headless(
        &rom_path,
        Vec::new(),
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;

    let saved = TasProject::load(&project_path)?;
    assert!(saved.verification_is_current("main")?);
    let replay = ReplayPlayer::load(&export_path)?;
    assert_eq!(replay.total_frames(), 2);
    assert_eq!(
        replay.peek_joypad_frames(0, 2),
        vec![
            zeff_emu_common::replay::ReplayJoypadFrame {
                buttons: 0x01,
                dpad: 0x04,
                buttons_p2: 0x02,
                dpad_p2: 0x08,
                ..Default::default()
            };
            2
        ]
    );
    Ok(())
}

#[test]
fn native_cli_two_pass_verifies_injected_direct_game_gear_input() -> Result<()> {
    use zeff_sega8_core::hardware::cartridge::{
        GameGearCartridgeIdentity, GameGearStandardMapperRam,
    };

    let directory = test_directory("tas-cli-game-gear")?;
    let rom_path = directory.path().join("game.gg");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    let rom = direct_game_gear_rom();
    std::fs::write(&rom_path, &rom)?;
    let loader = DirectGameGearTasExecutionLoader::new_with_catalog_entry(
        rom_path,
        GameGearCartridgeIdentity {
            sha256: zeff_firmware::sha256_bytes(&rom),
            source_len: rom.len(),
        },
        GameGearStandardMapperRam::Absent,
    );
    let mut project = loader.create_project()?;
    project.edit_transaction(|edit| {
        edit.insert_frames("main", 1, 1)?;
        edit.set_input_range(
            "main",
            0,
            2,
            TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 0x09,
                        dpad: 0x04,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        )
    })?;
    project.save_atomic(&project_path)?;
    run_tas_project_headless_with_plan(
        PrivateTasExecutionLoader::DirectGameGear(loader),
        &project_path,
        "main",
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;
    let saved = TasProject::load(&project_path)?;
    assert!(saved.verification_is_current("main")?);
    let replay = ReplayPlayer::load(&export_path)?;
    assert_eq!(replay.total_frames(), 2);
    assert_eq!(replay.peek_joypad_frames(0, 2)[0].buttons, 0x09);
    assert_eq!(replay.peek_joypad_frames(0, 2)[0].dpad, 0x04);
    Ok(())
}

#[test]
fn native_cli_replays_project_owned_game_gear_sram() -> Result<()> {
    use zeff_sega8_core::hardware::cartridge::{
        GameGearCartridgeIdentity, GameGearStandardMapperRam,
    };

    let directory = test_directory("tas-cli-game-gear-battery")?;
    let rom_path = directory.path().join("game.gg");
    let save_path = rom_path.with_extension("sav");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    let rom = direct_game_gear_rom();
    let initial = vec![0x39; 8 * 1024];
    let later = vec![0xC6; 8 * 1024];
    std::fs::write(&rom_path, &rom)?;
    std::fs::write(&save_path, &initial)?;
    let loader = DirectGameGearTasExecutionLoader::new_with_catalog_entry(
        rom_path,
        GameGearCartridgeIdentity {
            sha256: zeff_firmware::sha256_bytes(&rom),
            source_len: rom.len(),
        },
        GameGearStandardMapperRam::BatteryBacked8KiB,
    );
    let project = loader.create_project()?;
    project.save_atomic(&project_path)?;
    std::fs::write(&save_path, &later)?;
    run_tas_project_headless_with_plan(
        PrivateTasExecutionLoader::DirectGameGear(loader),
        &project_path,
        "main",
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;
    assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
    assert_eq!(ReplayPlayer::load(&export_path)?.total_frames(), 1);
    assert_eq!(std::fs::read(save_path)?, later);
    Ok(())
}

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_sg1000_two_pad_input() -> Result<()> {
    let directory = test_directory("tas-cli-sg1000")?;
    let rom_path = directory.path().join("game.sc");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    std::fs::write(&rom_path, vec![0x76; 32 * 1024])?;
    let loader = DirectSg1000TasExecutionLoader::new(rom_path.clone());
    let mut project = loader.create_project()?;
    project.edit_transaction(|edit| {
        edit.insert_frames("main", 1, 1)?;
        edit.set_input_range(
            "main",
            0,
            2,
            TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 0x01,
                        dpad: 0x04,
                    },
                    TasControllerInput {
                        buttons: 0x02,
                        dpad: 0x08,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        )
    })?;
    project.save_atomic(&project_path)?;

    run_tas_project_headless(
        &rom_path,
        Vec::new(),
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;

    let saved = TasProject::load(&project_path)?;
    assert!(saved.verification_is_current("main")?);
    let replay = ReplayPlayer::load(&export_path)?;
    assert_eq!(replay.total_frames(), 2);
    assert_eq!(
        replay.peek_joypad_frames(0, 2),
        vec![
            zeff_emu_common::replay::ReplayJoypadFrame {
                buttons: 0x01,
                dpad: 0x04,
                buttons_p2: 0x02,
                dpad_p2: 0x08,
                ..Default::default()
            };
            2
        ]
    );
    Ok(())
}

#[test]
fn native_cli_rejects_media_change_without_mutating_project() -> Result<()> {
    let directory = test_directory("tas-cli-interrupted")?;
    let rom_path = directory.path().join("game.nes");
    let project_path = directory.path().join("movie.ztas");
    let rom = executable_nes_rom();
    std::fs::write(&rom_path, &rom)?;
    project_for_rom(&rom_path, &rom, 1)?.save_atomic(&project_path)?;
    let before = std::fs::read(&project_path)?;
    let mut changed = rom;
    changed[16] ^= 1;
    std::fs::write(&rom_path, changed)?;

    let result = run_tas_project_headless(
        &rom_path,
        Vec::new(),
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            ..HeadlessOptions::default()
        },
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read(project_path)?, before);
    Ok(())
}

#[test]
fn public_tas_mutation_api_accounts_movie_and_presentation_edits() -> Result<()> {
    let directory = test_directory("tas-cli-edit-boundary")?;
    let rom_path = directory.path().join("game.nes");
    let rom = executable_nes_rom();
    let mut project = project_for_rom(&rom_path, &rom, 1)?;

    assert_eq!(project.edit_generation(), 0);
    assert_eq!(project.rerecord_count(), 0);
    project.edit_transaction(|edit| {
        edit.set_project_comment("presentation-only");
        edit.replace_markers(vec![TasMarker {
            id: "public-marker".to_owned(),
            branch_id: "main".to_owned(),
            cursor: 1,
            name: "End".to_owned(),
        }]);
        edit.replace_annotations(vec![TasAnnotation {
            id: "public-annotation".to_owned(),
            branch_id: "main".to_owned(),
            start: 0,
            length: 1,
            kind: "note".to_owned(),
            text: "Transaction-owned".to_owned(),
        }]);
        Ok(())
    })?;
    assert_eq!(project.project_comment(), "presentation-only");
    assert_eq!(project.markers()[0].id, "public-marker");
    assert_eq!(project.annotations()[0].id, "public-annotation");
    assert_eq!(project.edit_generation(), 1);
    assert_eq!(project.rerecord_count(), 0);

    project
        .edit_transaction(|edit| edit.set_input_range("main", 0, 1, TasInputFrame::default()))?;
    assert_eq!(
        project.branch("main").unwrap().input_at(0),
        TasInputFrame::default()
    );
    assert_eq!(project.edit_generation(), 2);
    assert_eq!(project.rerecord_count(), 1);
    Ok(())
}

#[test]
fn public_camera_asset_edits_commit_or_roll_back_with_their_movie() -> Result<()> {
    let directory = test_directory("tas-cli-camera-edit-boundary")?;
    let rom_path = directory.path().join("game.nes");
    let rom = executable_nes_rom();
    let mut project = project_for_rom(&rom_path, &rom, 1)?;
    let mut input = project.branch("main").unwrap().input_at(0);
    let mut digest = None;

    project.edit_transaction(|edit| {
        let asset = edit.insert_camera_asset(vec![1, 2, 3, 4]);
        digest = Some(asset);
        input.camera = TasCameraInput::Blob(asset);
        edit.set_input_range("main", 0, 1, input)
    })?;
    let digest = digest.expect("transaction should return its camera digest");
    assert_eq!(
        project.assets().get(&digest).map(Vec::as_slice),
        Some(&[1, 2, 3, 4][..])
    );
    assert_eq!(project.edit_generation(), 1);
    assert_eq!(project.rerecord_count(), 1);

    let before = project.clone();
    assert!(
        project
            .edit_transaction(|edit| {
                assert!(edit.remove_camera_asset(digest));
                Ok(())
            })
            .is_err()
    );
    assert_eq!(project, before);
    Ok(())
}

#[test]
fn native_cli_rejects_invalid_start_states_transactionally() -> Result<()> {
    let directory = test_directory("tas-cli-profile")?;
    let rom_path = directory.path().join("game.nes");
    let rom = executable_nes_rom();
    std::fs::write(&rom_path, &rom)?;

    for name in ["legacy", "oversized"] {
        let backend = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &rom_path,
            &rom_path,
            Some(rom.clone()),
            BackendLoadConfig::default(),
        )?
        .backend;
        let start_state = if name == "legacy" {
            let mut start_state = backend.encode_state_bytes()?;
            start_state[8..12].copy_from_slice(&10_u32.to_le_bytes());
            start_state
        } else if name == "oversized" {
            let mut start_state = Vec::new();
            start_state.extend_from_slice(&zeff_nes_core::save_state::NES_SAVE_STATE_MAGIC);
            start_state.extend_from_slice(
                &zeff_nes_core::save_state::NES_SAVE_STATE_FORMAT_VERSION.to_le_bytes(),
            );
            start_state.extend_from_slice(&u32::MAX.to_le_bytes());
            start_state
        } else {
            unreachable!()
        };
        let project = project_for_rom_with_start_state(&rom, 1, &backend, start_state)?;
        let project_path = directory.path().join(format!("{name}.ztas"));
        project.save_atomic(&project_path)?;
        let before = std::fs::read(&project_path)?;

        let result = run_tas_project_headless(
            &rom_path,
            Vec::new(),
            &HeadlessOptions {
                tas_project_path: Some(project_path.clone()),
                ..HeadlessOptions::default()
            },
        );
        assert!(result.is_err(), "{name} start state should be rejected");
        assert_eq!(std::fs::read(project_path)?, before);
    }
    Ok(())
}

#[test]
fn native_cli_executes_the_declared_zapper_profile() -> Result<()> {
    let directory = test_directory("tas-cli-zapper-profile")?;
    let rom_path = directory.path().join("game.nes");
    let project_path = directory.path().join("zapper.ztas");
    let rom = executable_nes_rom();
    std::fs::write(&rom_path, &rom)?;
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        Some(rom.clone()),
        BackendLoadConfig::default(),
    )?
    .backend;
    backend.set_zapper_state(true, false, false, Some((120, 80)));
    let start_state = backend.encode_state_bytes()?;
    let mut project = project_for_rom_with_start_state(&rom, 1, &backend, start_state)?;
    project.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                zapper: TasZapperInput {
                    enabled: true,
                    trigger: true,
                    hit: false,
                    screen_pos: Some([120, 80]),
                },
                ..TasInputFrame::default()
            },
        )
    })?;
    project.save_atomic(&project_path)?;

    run_tas_project_headless(
        &rom_path,
        Vec::new(),
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;

    assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
    Ok(())
}

#[test]
fn native_cli_rejects_non_direct_media_without_mutating_project() -> Result<()> {
    let directory = test_directory("tas-cli-fds")?;
    let rom_path = directory.path().join("game.nes");
    let disguised_path = directory.path().join("game.fds");
    let project_path = directory.path().join("movie.ztas");
    let rom = executable_nes_rom();
    std::fs::write(&rom_path, &rom)?;
    std::fs::write(&disguised_path, &rom)?;
    project_for_rom(&rom_path, &rom, 1)?.save_atomic(&project_path)?;
    let before = std::fs::read(&project_path)?;

    let result = run_tas_project_headless(
        &disguised_path,
        Vec::new(),
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            ..HeadlessOptions::default()
        },
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read(project_path)?, before);
    Ok(())
}

#[test]
fn bounded_nes_media_reader_rejects_oversized_file() -> Result<()> {
    let directory = test_directory("tas-cli-oversized")?;
    let path = directory.path().join("oversized.nes");
    std::fs::File::create(&path)?.set_len(MAX_NES_CARTRIDGE_BYTES + 1)?;
    assert!(read_nes_cartridge_bounded(&path).is_err());
    Ok(())
}

#[test]
fn canonical_nes_tas_loader_ignores_battery_sidecar() -> Result<()> {
    let directory = test_directory("tas-cli-isolated")?;
    let isolated_path = directory.path().join("isolated.nes");
    let baseline_path = directory.path().join("baseline.nes");
    let mut rom = executable_nes_rom();
    rom[6] = 0x02;
    rom[7] = 0x10;
    rom[8] = 1;
    std::fs::write(&isolated_path, &rom)?;
    std::fs::write(&baseline_path, &rom)?;

    let mut persistent_source =
        zeff_nes_core::emulator::Emulator::new(&rom, zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE)?;
    persistent_source.load_persistent_data(&vec![0xA5; 8 * 1024])?;
    let sidecar = persistent_source
        .dump_persistent_data()
        .context("battery fixture should expose persistent data")?;
    std::fs::write(isolated_path.with_extension("sav"), sidecar)?;

    let baseline = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &baseline_path,
        &baseline_path,
        Some(rom.clone()),
        BackendLoadConfig::default(),
    )?
    .backend
    .encode_state_bytes()?;
    let isolated = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &isolated_path,
        &isolated_path,
        Some(rom),
        BackendLoadConfig {
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )?
    .backend
    .encode_state_bytes()?;
    assert_eq!(isolated, baseline);
    Ok(())
}

#[test]
fn nes_cartridge_scope_rejects_every_unowned_timeline_domain() -> Result<()> {
    let directory = test_directory("tas-cli-failure")?;
    let rom_path = directory.path().join("game.nes");
    let rom = executable_nes_rom();
    let base = project_for_rom(&rom_path, &rom, 1)?;

    let mut cases = Vec::new();
    let mut project = base.clone();
    let mut input = project.branch("main").unwrap().input_at(0);
    input.players[2].buttons = 1;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    cases.push(project);
    let mut project = base.clone();
    let mut input = project.branch("main").unwrap().input_at(0);
    input.zapper.enabled = true;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    cases.push(project);
    let mut project = base.clone();
    let mut input = project.branch("main").unwrap().input_at(0);
    input.tilt_x_bits = 1;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    cases.push(project);
    let mut project = base.clone();
    let mut camera_input = project.branch("main").unwrap().input_at(0);
    project.edit_transaction(|edit| {
        camera_input.camera = TasCameraInput::Blob(edit.insert_camera_asset(vec![1]));
        edit.set_input_range("main", 0, 1, camera_input)
    })?;
    cases.push(project);
    let mut project = base.clone();
    project.edit_transaction(|edit| {
        edit.replace_branch_events(
            "main",
            vec![zeff_emu_common::replay::ReplayEvent::FdsDiskSide { frame: 0, side: 0 }],
        )
    })?;
    cases.push(project);
    let mut replay_start = base.replay_start().clone();
    replay_start.wonder_swan_link_tick = Some(0);
    let branch = base.branch("main").unwrap();
    let project = TasProject::new(
        base.project_id(),
        base.identity().clone(),
        base.start_state().to_vec(),
        replay_start,
        TasInitialBranch {
            id: branch.id().to_owned(),
            name: branch.name().to_owned(),
            frame_count: branch.frame_count(),
            input_spans: branch.input_spans().to_vec(),
            events: branch.events().to_vec(),
        },
        base.assets().clone(),
    )?;
    cases.push(project);

    for project in cases {
        assert!(
            DirectNesTasExecutionLoader::validate_project_branch_scope(&project, "main").is_err()
        );
    }
    Ok(())
}

#[test]
fn export_failure_leaves_durable_verification() -> Result<()> {
    let directory = test_directory("tas-cli-occupied-export")?;
    let rom_path = directory.path().join("game.nes");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("occupied.zrpl");
    let rom = executable_nes_rom();
    std::fs::write(&rom_path, &rom)?;
    project_for_rom(&rom_path, &rom, 1)?.save_atomic(&project_path)?;
    std::fs::write(&export_path, b"keep")?;

    let result = run_tas_project_headless(
        &rom_path,
        Vec::new(),
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    );
    let error = result.expect_err("an occupied replay export path must fail after saving");
    assert!(
        format!("{error:#}").contains("refusing to overwrite existing replay"),
        "unexpected export failure: {error:#}"
    );
    assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
    assert_eq!(std::fs::read(export_path)?, b"keep");
    Ok(())
}

mod gb_tests;
mod gbc_tests;
mod nes_tests;
mod pce_cd_tests;
mod ws_tests;

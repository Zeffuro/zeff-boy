use anyhow::Result;
use zeff_sega8_core::hardware::cartridge::{GameGearCartridgeIdentity, GameGearStandardMapperRam};

use super::*;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasEditorSession, TasInputFrame,
    TasSeekStateCache,
};
use crate::test_support::write_zip;

pub(crate) fn game_gear_rom() -> Vec<u8> {
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

pub(crate) fn injected_loader(
    label: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectGameGearTasExecutionLoader,
    Vec<u8>,
)> {
    let directory = crate::test_support::test_directory(label)?;
    let path = directory.path().join("game.gg");
    let rom = game_gear_rom();
    std::fs::write(&path, &rom)?;
    let identity = GameGearCartridgeIdentity {
        sha256: zeff_firmware::sha256_bytes(&rom),
        source_len: rom.len(),
    };
    Ok((
        directory,
        DirectGameGearTasExecutionLoader::new_with_catalog_entry(
            path,
            identity,
            GameGearStandardMapperRam::Absent,
        ),
        rom,
    ))
}

#[test]
fn unknown_game_gear_media_remains_rejected() -> Result<()> {
    let (directory, _, _) = injected_loader("tas-direct-game-gear-unknown")?;
    assert!(
        DirectGameGearTasExecutionLoader::new(directory.path().join("game.gg"))
            .create_project()
            .is_err()
    );
    assert!(
        super::super::select_private_tas_execution_loader(
            directory.path().join("game.gg"),
            ActiveSystem::GameGear,
            Vec::new(),
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn confirmed_no_save_board_choice_is_durable_and_reconstructed_for_an_unknown_rom() -> Result<()> {
    let (directory, _, _) = injected_loader("tas-direct-game-gear-confirmed-no-save")?;
    let source_path = directory.path().join("game.gg");
    let loader = DirectGameGearTasExecutionLoader::new_with_confirmed_no_cartridge_save_memory(
        source_path.clone(),
    );
    assert!(
        loader
            .requires_confirmed_no_cartridge_save_memory()
            .is_err()
    );
    let project = loader.create_project()?;
    assert_eq!(
        super::super::direct_game_gear::direct_game_gear_tas_board_choice(project.identity())?,
        super::super::direct_game_gear::DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory
    );
    assert_ne!(
        project.identity().sync_config_sha256,
        super::super::direct_game_gear_tas_sync_config_sha256()
    );
    DirectGameGearTasExecutionLoader::new(source_path.clone()).load_editor_engine(&project)?;
    assert!(matches!(
        super::super::select_private_tas_execution_attachment(
            Some(source_path.clone()),
            None,
            Some(ActiveSystem::GameGear),
            Vec::new(),
            Some(&project),
        ),
        crate::tas_project::TasEditorExecutionAttachment::Available(_)
    ));
    let plan = super::super::select_private_tas_execution_loader_for_project(
        source_path,
        ActiveSystem::GameGear,
        Vec::new(),
        &project,
    )?;
    assert_eq!(
        plan.load_session(project.start_state())?.identity(),
        project.identity()
    );
    Ok(())
}

#[test]
fn unclassified_game_gear_rom_requires_explicit_no_save_confirmation() -> Result<()> {
    let (directory, _, _) = injected_loader("tas-direct-game-gear-confirmation")?;
    assert!(matches!(
        super::super::select_private_tas_execution_attachment(
            Some(directory.path().join("game.gg")),
            None,
            Some(ActiveSystem::GameGear),
            Vec::new(),
            None,
        ),
        crate::tas_project::TasEditorExecutionAttachment::Unavailable(_)
    ));
    assert!(
        DirectGameGearTasExecutionLoader::new(directory.path().join("game.gg"))
            .requires_confirmed_no_cartridge_save_memory()?
    );
    Ok(())
}

#[test]
fn confirmed_no_save_zip_member_binds_archive_member_and_effective_media() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-direct-game-gear-zip")?;
    let archive_path = directory.path().join("games.zip");
    let first = game_gear_rom();
    let mut selected = first.clone();
    selected[0] ^= 1;
    let archive_bytes = write_zip(
        &archive_path,
        &[("first.gg", &first), ("folder/selected.gg", &selected)],
    )?;
    assert!(
        DirectGameGearTasExecutionLoader::new_zip(archive_path.clone(), None, false)
            .create_project()
            .is_err()
    );
    assert!(
        DirectGameGearTasExecutionLoader::new_zip(archive_path.clone(), None, true)
            .create_project()
            .is_err()
    );
    let loader = DirectGameGearTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/selected.gg")),
        true,
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256,
        crate::tas_project::TasDigest::from_bytes(&archive_bytes)
    );
    assert_eq!(
        project.identity().effective_media_sha256,
        crate::tas_project::TasDigest::from_bytes(&selected)
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_game_gear::zip_game_gear_tas_sync_config_sha256(
            super::super::direct_game_gear::DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory,
            "folder/selected.gg",
        )
    );
    let reopened =
        DirectGameGearTasExecutionLoader::new_zip_for_project(archive_path.clone(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );

    write_zip(
        &archive_path,
        &[
            ("first.gg", &first),
            ("folder/selected.gg", &selected),
            ("note.txt", b"changed"),
        ],
    )?;
    assert!(DirectGameGearTasExecutionLoader::new_zip_for_project(archive_path, &project).is_err());
    Ok(())
}

#[test]
fn confirmed_no_save_replacement_keeps_the_previous_project_backup() -> Result<()> {
    let (directory, _, _) = injected_loader("tas-direct-game-gear-confirmed-replace")?;
    let source_path = directory.path().join("game.gg");
    let loader =
        DirectGameGearTasExecutionLoader::new_with_confirmed_no_cartridge_save_memory(source_path);
    let project_path = directory.path().join("movie.ztas");
    let original = loader.create_project_file(&project_path)?;
    let replacement = loader.replace_project_file(&project_path)?;
    assert_eq!(TasProject::load(&project_path)?, replacement);
    assert_eq!(
        TasProject::load(&TasProject::backup_path(&project_path)?)?,
        original
    );
    Ok(())
}

#[test]
fn injected_absent_board_creates_and_executes_pad_and_start() -> Result<()> {
    let (directory, loader, _) = injected_loader("tas-direct-game-gear-isolated")?;
    let mut project = loader.create_project()?;
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&project)?,
        crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge
    );
    project.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
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
    let mut engine = loader.load_editor_engine(&project)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let outcome = engine.seek(&mut editor, 1)?;
    assert!(outcome.reached_target());
    let inspection = zeff_sega8_core::save_state::inspect_current_native_game_gear_tas_state(
        &engine.backend().sega8().unwrap().emu,
        &engine.backend().encode_state_bytes()?,
    )?;
    assert!(inspection.start_pressed);
    assert_eq!(inspection.controller_raw[0], 0xEE);
    assert_eq!(inspection.controller_raw[1], 0xFF);
    Ok(())
}

#[test]
fn replay_round_trip_preserves_game_gear_start_and_pad_input() -> Result<()> {
    let (directory, loader, _) = injected_loader("tas-direct-game-gear-replay")?;
    let mut project = loader.create_project()?;
    let input = TasInputFrame {
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
    };
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let manual_path = directory.path().join("source.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("replay-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let plan = super::super::PrivateTasExecutionLoader::DirectGameGear(loader.clone());
    let replay_path = directory.path().join("movie.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;
    let imported_path = directory.path().join("imported.ztas");
    let imported = plan.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectGameGearCartridge
    );
    Ok(())
}

#[test]
fn battery_catalog_entry_imports_sram_once() -> Result<()> {
    let (directory, _, rom) = injected_loader("tas-direct-game-gear-battery")?;
    let source_path = directory.path().join("game.gg");
    let save_path = source_path.with_extension("sav");
    let initial = vec![0x3C; 8 * 1024];
    std::fs::write(&save_path, &initial)?;
    let battery = DirectGameGearTasExecutionLoader::new_with_catalog_entry(
        source_path,
        GameGearCartridgeIdentity {
            sha256: zeff_firmware::sha256_bytes(&rom),
            source_len: rom.len(),
        },
        GameGearStandardMapperRam::BatteryBacked8KiB,
    );
    let project = battery.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        crate::tas_project::TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial))
    );
    assert_eq!(
        super::super::direct_game_gear::direct_game_gear_tas_board_choice(project.identity())?,
        super::super::direct_game_gear::DirectGameGearTasBoardChoice::CataloguedBattery8KiB
    );
    std::fs::write(&save_path, vec![0xA5; 8 * 1024])?;
    let engine = battery.load_editor_engine(&project)?;
    assert_eq!(
        engine
            .backend()
            .sega8()
            .unwrap()
            .emu
            .dump_battery_sram()
            .unwrap(),
        initial
    );
    assert!(
        super::super::validate_direct_game_gear_tas_execution_runtime(engine.backend(), false,)
            .is_err()
    );
    assert_eq!(std::fs::read(save_path)?, vec![0xA5; 8 * 1024]);
    Ok(())
}

#[test]
fn zip_battery_project_uses_archive_sidecar_and_rejects_embedded_save() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-game-gear-zip-battery")?;
    let archive_path = directory.path().join("games.zip");
    let rom = game_gear_rom();
    let member_path = archive_path.join("folder/game.gg");
    let identity = GameGearCartridgeIdentity {
        sha256: zeff_firmware::sha256_bytes(&rom),
        source_len: rom.len(),
    };
    write_zip(&archive_path, &[("folder/game.gg", &rom)])?;
    let save_path = archive_path.with_extension("sav");
    let initial = vec![0x5A; 8 * 1024];
    std::fs::write(&save_path, &initial)?;
    let loader = DirectGameGearTasExecutionLoader::new_zip_with_catalog_entry(
        archive_path.clone(),
        member_path.clone(),
        identity,
        GameGearStandardMapperRam::BatteryBacked8KiB,
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        crate::tas_project::TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial))
    );
    std::fs::write(&save_path, vec![0xC3; 8 * 1024])?;
    let engine = loader.load_editor_engine(&project)?;
    assert_eq!(
        engine
            .backend()
            .sega8()
            .unwrap()
            .emu
            .dump_battery_sram()
            .unwrap(),
        initial
    );
    write_zip(
        &archive_path,
        &[("folder/game.gg", &rom), ("folder/game.sav", &initial)],
    )?;
    let embedded = DirectGameGearTasExecutionLoader::new_zip_with_catalog_entry(
        archive_path,
        member_path,
        identity,
        GameGearStandardMapperRam::BatteryBacked8KiB,
    );
    assert!(embedded.create_project().is_err());
    Ok(())
}

#[test]
fn changed_media_is_rejected() -> Result<()> {
    let (_directory, loader, mut rom) = injected_loader("tas-direct-game-gear-reject")?;
    let project = loader.create_project()?;
    rom[0] ^= 1;
    std::fs::write(&loader.source_path, rom)?;
    assert!(loader.load_session(project.start_state()).is_err());
    Ok(())
}

use anyhow::Result;

use super::*;
use crate::test_support::write_zip;

#[test]
fn selected_zip_member_creates_reopens_and_executes() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-nes-zip-selected")?;
    let archive_path = directory.path().join("games.zip");
    let first = crate::test_support::build_nes_test_rom();
    let mut selected = first.clone();
    selected[16] ^= 0x01;
    let archive_bytes = write_zip(
        &archive_path,
        &[("first.nes", &first), ("folder/selected.nes", &selected)],
    )?;
    let rom_path = archive_path.join("folder/selected.nes");
    let loader =
        DirectNesTasExecutionLoader::new_zip(archive_path.clone(), Some(rom_path), Vec::new());

    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&archive_bytes)
    );
    assert_eq!(
        project.identity().effective_media_sha256,
        TasDigest::from_bytes(&selected)
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        zip_nes_tas_sync_config_sha256("folder/selected.nes")
    );

    let reopened =
        DirectNesTasExecutionLoader::new_zip_for_project(archive_path, Vec::new(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );
    let mut engine = reopened.load_editor_engine(&project)?;
    let manual_path = directory.path().join("movie.ztas");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &manual_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let seek_cache =
        crate::tas_project::TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;
    assert_eq!(engine.seek(&mut editor, 1)?.cursor, 1);
    Ok(())
}

#[test]
fn zip_selection_is_explicit_and_archive_changes_reject() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-nes-zip-reject")?;
    let archive_path = directory.path().join("games.zip");
    let first = crate::test_support::build_nes_test_rom();
    let mut second = first.clone();
    second[16] ^= 0x01;
    write_zip(
        &archive_path,
        &[("first.nes", &first), ("second.nes", &second)],
    )?;
    assert!(
        DirectNesTasExecutionLoader::new_zip(archive_path.clone(), None, Vec::new())
            .create_project()
            .is_err()
    );
    let loader = DirectNesTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("second.nes")),
        Vec::new(),
    );
    let project = loader.create_project()?;

    write_zip(
        &archive_path,
        &[
            ("first.nes", &first),
            ("second.nes", &second),
            ("note.txt", b"changed"),
        ],
    )?;
    assert!(
        DirectNesTasExecutionLoader::new_zip_for_project(archive_path, Vec::new(), &project)
            .is_err()
    );
    Ok(())
}

#[test]
fn oversized_archives_reject() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-nes-zip-bounds")?;
    let oversized_path = directory.path().join("oversized.zip");
    let oversized = std::fs::File::create(&oversized_path)?;
    oversized.set_len(MAX_NES_ZIP_BYTES + 1)?;
    assert!(
        DirectNesTasExecutionLoader::new_zip(oversized_path, None, Vec::new())
            .create_project()
            .is_err()
    );
    Ok(())
}

#[test]
fn direct_battery_project_imports_sram_once() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-nes-battery")?;
    let rom_path = directory.path().join("game.nes");
    let save_path = rom_path.with_extension("sav");
    let rom = crate::test_support::build_nes_battery_test_rom();
    let initial_sram = crate::test_support::nes_battery_test_bytes(&rom, 0xA5);
    std::fs::write(&rom_path, &rom)?;
    std::fs::write(&save_path, &initial_sram)?;

    let loader = DirectNesTasExecutionLoader::new(rom_path, Vec::new());
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial_sram))
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        direct_nes_battery_tas_sync_config_sha256()
    );

    let changed_sidecar = vec![0x3C; initial_sram.len()];
    std::fs::write(&save_path, &changed_sidecar)?;
    let mut engine = loader.load_editor_engine(&project)?;
    let manual_path = directory.path().join("movie.ztas");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &manual_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let seek_cache =
        crate::tas_project::TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;
    assert_eq!(engine.seek(&mut editor, 1)?.cursor, 1);
    assert_eq!(std::fs::read(save_path)?, changed_sidecar);
    Ok(())
}

#[test]
fn zip_battery_project_uses_the_archive_sidecar() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-nes-zip-battery")?;
    let archive_path = directory.path().join("games.zip");
    let save_path = archive_path.with_extension("sav");
    let rom = crate::test_support::build_nes_battery_test_rom();
    let initial_sram = crate::test_support::nes_battery_test_bytes(&rom, 0x6D);
    write_zip(&archive_path, &[("folder/game.nes", &rom)])?;
    std::fs::write(&save_path, &initial_sram)?;

    let loader = DirectNesTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/game.nes")),
        Vec::new(),
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial_sram))
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        zip_nes_battery_tas_sync_config_sha256("folder/game.nes")
    );

    let changed_sidecar = vec![0xC7; initial_sram.len()];
    std::fs::write(&save_path, &changed_sidecar)?;
    let reopened =
        DirectNesTasExecutionLoader::new_zip_for_project(archive_path, Vec::new(), &project)?;
    let mut engine = reopened.load_editor_engine(&project)?;
    let manual_path = directory.path().join("movie.ztas");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &manual_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let seek_cache =
        crate::tas_project::TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;
    assert_eq!(engine.seek(&mut editor, 1)?.cursor, 1);
    assert_eq!(std::fs::read(save_path)?, changed_sidecar);
    Ok(())
}

#[test]
fn zip_battery_project_rejects_embedded_sram() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-nes-zip-embedded-save")?;
    let archive_path = directory.path().join("games.zip");
    let rom = crate::test_support::build_nes_battery_test_rom();
    let sidecar = crate::test_support::nes_battery_test_bytes(&rom, 0x91);
    write_zip(
        &archive_path,
        &[("folder/game.nes", &rom), ("folder/game.sav", &sidecar)],
    )?;
    assert!(
        DirectNesTasExecutionLoader::new_zip(
            archive_path.clone(),
            Some(archive_path.join("folder/game.nes")),
            Vec::new(),
        )
        .create_project()
        .is_err()
    );
    Ok(())
}

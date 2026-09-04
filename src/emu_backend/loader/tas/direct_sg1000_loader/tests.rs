use anyhow::Result;
use zeff_emu_common::save_ram::SaveRamKind;

use super::*;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasEditorSession, TasInputFrame,
    TasSeekStateCache,
};
use crate::test_support::write_zip;

fn sg_rom() -> Vec<u8> {
    vec![0x76; 32 * 1024]
}

fn loader(
    label: &str,
    extension: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectSg1000TasExecutionLoader,
    Vec<u8>,
)> {
    let directory = crate::test_support::test_directory(label)?;
    let path = directory.path().join(format!("game.{extension}"));
    let rom = sg_rom();
    std::fs::write(&path, &rom)?;
    Ok((directory, DirectSg1000TasExecutionLoader::new(path), rom))
}

fn two_pad_input() -> TasInputFrame {
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
    }
}

#[test]
fn creates_and_executes_direct_sg_and_sc_two_pad_projects() -> Result<()> {
    for extension in ["sg", "sc"] {
        let (directory, loader, rom) =
            loader(&format!("tas-direct-sg1000-{extension}"), extension)?;
        let mut project = loader.create_project()?;
        assert_eq!(
            project.project_id(),
            format!(
                "sg1000-{}",
                crate::tas_project::TasDigest::from_bytes(&rom).to_hex()
            )
        );
        assert_eq!(project.identity().devices.len(), 2);
        assert!(super::super::validate_direct_sg1000_tas_project_identity(&project).is_ok());
        assert_eq!(
            super::super::classify_direct_tas_execution_profile(&project)?,
            crate::emu_thread::TasExecutionProfile::DirectSg1000Cartridge
        );
        let input = two_pad_input();
        project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
        let mut engine = loader.load_editor_engine(&project)?;
        let manual_path = directory.path().join("manual.ztas");
        let autosaves =
            TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
        let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
        let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
        let outcome = engine.seek(&mut editor, 1)?;
        let (mut expected, _) = loader.load_fresh_backend()?;
        expected.set_input(0x01, 0x04);
        expected.set_input_p2(0x02, 0x08);
        expected.step_frame();
        assert!(outcome.reached_target());
        assert_eq!(engine.backend().save_ram_kind(), SaveRamKind::None);
        assert_eq!(
            engine.backend().encode_state_bytes()?,
            expected.encode_state_bytes()?
        );
    }
    Ok(())
}

#[test]
fn runtime_rejects_path_forced_mapper_and_host_configuration() -> Result<()> {
    let (directory, loader, _) = loader("tas-direct-sg1000-runtime", "sg")?;
    let (backend, _) = loader.load_fresh_backend()?;
    assert!(super::super::validate_direct_sg1000_tas_runtime(&backend, false).is_ok());
    assert!(super::super::validate_direct_sg1000_tas_runtime(&backend, true).is_err());

    let tagged_path = directory.path().join("game [mapper=korean].sg");
    let tagged_backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
        ActiveSystem::Sg1000,
        &tagged_path,
        sg_rom(),
        BackendLoadConfig::default(),
    )?
    .backend;
    assert_eq!(tagged_backend.save_ram_kind(), SaveRamKind::None);
    assert!(super::super::validate_direct_sg1000_tas_runtime(&tagged_backend, false).is_err());

    for system in [ActiveSystem::MasterSystem, ActiveSystem::GameGear] {
        let backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
            system,
            &loader.source_path,
            sg_rom(),
            BackendLoadConfig::default(),
        )?
        .backend;
        assert!(super::super::validate_direct_sg1000_tas_runtime(&backend, false).is_err());
    }

    for config in [
        BackendLoadConfig {
            sample_rate: Some(44_100),
            ..BackendLoadConfig::default()
        },
        BackendLoadConfig {
            initial_input: Some((1, 0)),
            ..BackendLoadConfig::default()
        },
    ] {
        let backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
            ActiveSystem::Sg1000,
            &loader.source_path,
            sg_rom(),
            config,
        )?
        .backend;
        assert!(super::super::validate_direct_sg1000_tas_runtime(&backend, false).is_err());
    }
    Ok(())
}

#[test]
fn opening_refuses_changed_media_and_wrong_extension() -> Result<()> {
    let (directory, loader, mut rom) = loader("tas-direct-sg1000-identity", "sg")?;
    let project = loader.create_project()?;
    rom[0] ^= 1;
    std::fs::write(&loader.source_path, rom)?;
    assert!(loader.load_session(project.start_state()).is_err());
    let wrong = directory.path().join("game.sms");
    std::fs::write(&wrong, sg_rom())?;
    assert!(
        DirectSg1000TasExecutionLoader::new(wrong)
            .create_project()
            .is_err()
    );
    Ok(())
}

#[test]
fn selected_sg1000_zip_member_binds_archive_member_and_effective_media() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-direct-sg1000-zip")?;
    let archive_path = directory.path().join("games.zip");
    let first = sg_rom();
    let mut selected = first.clone();
    selected[0] ^= 1;
    let archive_bytes = write_zip(
        &archive_path,
        &[("first.sg", &first), ("folder/selected.sc", &selected)],
    )?;
    assert!(
        DirectSg1000TasExecutionLoader::new_zip(archive_path.clone(), None)
            .create_project()
            .is_err()
    );
    let loader = DirectSg1000TasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/selected.sc")),
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
        super::super::direct_sg1000::zip_sg1000_tas_sync_config_sha256("folder/selected.sc")
    );
    let reopened =
        DirectSg1000TasExecutionLoader::new_zip_for_project(archive_path.clone(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );

    write_zip(
        &archive_path,
        &[
            ("first.sg", &first),
            ("folder/selected.sc", &selected),
            ("note.txt", b"changed"),
        ],
    )?;
    assert!(DirectSg1000TasExecutionLoader::new_zip_for_project(archive_path, &project).is_err());
    Ok(())
}

#[test]
fn branch_scope_rejects_unowned_input_and_events() -> Result<()> {
    let (_directory, loader, _) = loader("tas-direct-sg1000-scope", "sg")?;
    let mut extra_input = loader.create_project()?;
    let mut input = two_pad_input();
    input.players[2].buttons = 1;
    extra_input.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    assert!(
        DirectSg1000TasExecutionLoader::validate_project_branch_scope(&extra_input, "main")
            .is_err()
    );

    let mut event = loader.create_project()?;
    event.edit_transaction(|edit| {
        edit.replace_branch_events(
            "main",
            vec![zeff_emu_common::replay::ReplayEvent::FdsDiskSide { frame: 0, side: 0 }],
        )
    })?;
    assert!(DirectSg1000TasExecutionLoader::validate_project_branch_scope(&event, "main").is_err());
    Ok(())
}

#[test]
fn replay_export_and_import_preserve_direct_sg1000_two_pad_input() -> Result<()> {
    let (directory, loader, _) = loader("tas-direct-sg1000-replay", "sc")?;
    let mut project = loader.create_project()?;
    let input = two_pad_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let manual_path = directory.path().join("source.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("replay-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let plan = super::super::PrivateTasExecutionLoader::DirectSg1000(loader.clone());
    let replay_path = directory.path().join("movie.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;
    let imported_path = directory.path().join("imported.ztas");
    let imported = plan.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectSg1000Cartridge
    );
    Ok(())
}

use super::*;
use std::sync::atomic::AtomicBool;

fn synthetic_nes_rom() -> Vec<u8> {
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

#[test]
fn new_project_is_deterministic_neutral_and_immediately_executable() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-loader-create")?;
    let source_path = directory.path().join("game.nes");
    let source_bytes = synthetic_nes_rom();
    std::fs::write(&source_path, &source_bytes)?;
    let loader = DirectNesTasExecutionLoader::new(source_path, Vec::new());

    let first = loader.create_project()?;
    let second = loader.create_project()?;

    assert_eq!(first, second);
    assert_eq!(first.encode()?, second.encode()?);
    assert_eq!(
        first.project_id(),
        format!("nes-{}", TasDigest::from_bytes(&source_bytes).to_hex())
    );
    assert_eq!(first.edit_generation(), 0);
    assert_eq!(first.rerecord_count(), 0);
    assert_eq!(first.source_replay_sha256(), None);
    assert_eq!(first.replay_start(), &ReplayStartMetadata::default());
    assert!(first.assets().is_empty());
    assert_eq!(first.active_branch_id(), "main");
    assert_eq!(first.branches().len(), 1);
    let branch = &first.branches()[0];
    assert_eq!(branch.id(), "main");
    assert_eq!(branch.name(), "Main");
    assert_eq!(branch.frame_count(), 1);
    assert!(branch.input_spans().is_empty());
    assert_eq!(branch.input_at(0), Default::default());
    assert!(branch.events().is_empty());
    assert!(branch.verification().is_none());
    assert_eq!(
        first.identity().source_media_sha256,
        TasDigest::from_bytes(&source_bytes)
    );
    assert_eq!(
        first.identity().start_state_sha256,
        TasDigest::from_bytes(first.start_state())
    );

    DirectNesTasExecutionLoader::validate_project_branch_scope(&first, "main")?;
    let session = loader.load_session(first.start_state())?;
    assert_eq!(session.identity(), first.identity());
    loader.load_editor_engine(&first)?;
    Ok(())
}

#[test]
fn repair_backend_keeps_direct_provenance_and_preloaded_copy_stays_ineligible() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-loader-repair-provenance")?;
    let source_path = directory.path().join("game.nes");
    let source_bytes = synthetic_nes_rom();
    std::fs::write(&source_path, &source_bytes)?;
    let loader = DirectNesTasExecutionLoader::new(source_path.clone(), Vec::new());
    let project = loader.create_project()?;

    let repair_backend = loader.load_editor_engine(&project)?.into_backend();
    assert!(
        crate::emu_thread::build_tas_repair_witness(
            &repair_backend,
            crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
        )
        .is_ok()
    );

    let preloaded = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &source_path,
        &source_path,
        Some(source_bytes),
        BackendLoadConfig {
            apply_mods: false,
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )?
    .backend;
    assert_eq!(
        crate::emu_thread::build_tas_repair_witness(
            &preloaded,
            crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
        ),
        Err(crate::emu_thread::TasControlAcquireRejectedReason::DirectNesFileRequired)
    );
    Ok(())
}

#[test]
fn new_project_file_is_valid_and_never_replaces_an_occupied_target() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-loader-create-file")?;
    let source_path = directory.path().join("game.nes");
    std::fs::write(&source_path, synthetic_nes_rom())?;
    let loader = DirectNesTasExecutionLoader::new(source_path, Vec::new());
    let project_path = directory.path().join("movie.ztas");

    let created = loader.create_project_file(&project_path)?;
    assert_eq!(TasProject::load(&project_path)?, created);

    std::fs::write(&project_path, b"occupied")?;
    let before = std::fs::read(&project_path)?;
    assert!(loader.create_project_file(&project_path).is_err());
    assert_eq!(std::fs::read(&project_path)?, before);
    Ok(())
}

#[test]
fn confirmed_replacement_keeps_the_previous_project_as_backup() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-loader-replace-file")?;
    let old_source_path = directory.path().join("old.nes");
    std::fs::write(&old_source_path, synthetic_nes_rom())?;
    let old_loader = DirectNesTasExecutionLoader::new(old_source_path, Vec::new());
    let project_path = directory.path().join("movie.ztas");
    let old_project = old_loader.create_project_file(&project_path)?;

    let new_source_path = directory.path().join("new.nes");
    let mut new_rom = synthetic_nes_rom();
    *new_rom.last_mut().expect("synthetic ROM is non-empty") = 0xA5;
    std::fs::write(&new_source_path, new_rom)?;
    let new_loader = DirectNesTasExecutionLoader::new(new_source_path, Vec::new());
    let new_project = new_loader.replace_project_file(&project_path)?;

    assert_ne!(new_project.project_id(), old_project.project_id());
    assert_eq!(TasProject::load(&project_path)?, new_project);
    assert_eq!(
        TasProject::load(&TasProject::backup_path(&project_path)?)?,
        old_project
    );

    std::fs::write(&project_path, b"invalid occupied project")?;
    let invalid_bytes = std::fs::read(&project_path)?;
    assert!(new_loader.replace_project_file(&project_path).is_err());
    assert_eq!(std::fs::read(&project_path)?, invalid_bytes);
    Ok(())
}

#[test]
fn new_project_rejects_non_project_destinations_without_publishing() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-loader-create-extension")?;
    let source_path = directory.path().join("game.nes");
    std::fs::write(&source_path, synthetic_nes_rom())?;
    let loader = DirectNesTasExecutionLoader::new(source_path, Vec::new());
    let destination = directory.path().join("movie.bin");

    assert!(loader.create_project_file(&destination).is_err());
    assert!(!destination.exists());
    Ok(())
}

#[test]
fn replay_import_uses_the_loaded_game_identity_and_publishes_a_project() -> Result<()> {
    use zeff_emu_common::replay::{ReplayJoypadFrame, ReplayMetadata, ReplayRecorder};

    let directory = crate::test_support::test_directory("tas-loader-import-replay")?;
    let source_path = directory.path().join("game.nes");
    std::fs::write(&source_path, synthetic_nes_rom())?;
    let loader = DirectNesTasExecutionLoader::new(source_path, Vec::new());
    let baseline = loader.create_project()?;
    let replay_path = directory.path().join("run.zrpl");
    let project_path = directory.path().join("run.ztas");
    let metadata = ReplayMetadata {
        system: Some(baseline.identity().system.clone()),
        core_family: Some(baseline.identity().core_family.clone()),
        rom_sha256: Some(baseline.identity().effective_media_sha256.0),
        ..ReplayMetadata::default()
    };
    let mut recorder = ReplayRecorder::new_with_metadata(
        PathBuf::new(),
        baseline.start_state().to_vec(),
        metadata,
    );
    recorder.record_joypad_frame(ReplayJoypadFrame::p1(1, 0));
    std::fs::write(&replay_path, recorder.into_bytes()?)?;

    let imported = PrivateTasExecutionLoader::DirectNes(loader).import_replay_file(
        &replay_path,
        &project_path,
        false,
    )?;

    assert_eq!(TasProject::load(&project_path)?, imported);
    assert_eq!(imported.identity(), baseline.identity());
    assert_eq!(imported.branches()[0].frame_count(), 1);
    assert_eq!(imported.branches()[0].input_at(0).players[0].buttons, 1);
    assert_eq!(
        imported.source_replay_sha256(),
        Some(TasDigest::from_bytes(&std::fs::read(&replay_path)?))
    );
    Ok(())
}

#[test]
fn replay_import_preserves_the_direct_nes_zapper_profile() -> Result<()> {
    use zeff_emu_common::replay::{
        ReplayJoypadFrame, ReplayMetadata, ReplayRecorder, ReplayZapperFrame,
    };

    let directory = crate::test_support::test_directory("tas-loader-import-zapper")?;
    let source_path = directory.path().join("game.nes");
    std::fs::write(&source_path, synthetic_nes_rom())?;
    let loader = DirectNesTasExecutionLoader::new(source_path, Vec::new());
    let (mut backend, _) = loader.load_fresh_backend()?;
    backend.set_zapper_state(true, false, false, Some((120, 80)));
    let start_state = backend.encode_state_bytes()?;
    let baseline = loader.create_project()?;
    let metadata = ReplayMetadata {
        system: Some(baseline.identity().system.clone()),
        core_family: Some(baseline.identity().core_family.clone()),
        rom_sha256: Some(baseline.identity().effective_media_sha256.0),
        ..ReplayMetadata::default()
    };
    let mut recorder = ReplayRecorder::new_with_metadata(PathBuf::new(), start_state, metadata);
    recorder.record_joypad_frame(ReplayJoypadFrame {
        zapper: ReplayZapperFrame {
            enabled: true,
            trigger: true,
            hit: false,
            screen_pos: Some((120, 80)),
        },
        ..ReplayJoypadFrame::default()
    });
    let replay_path = directory.path().join("zapper.zrpl");
    let project_path = directory.path().join("zapper.ztas");
    std::fs::write(&replay_path, recorder.into_bytes()?)?;

    let imported = PrivateTasExecutionLoader::DirectNes(loader.clone()).import_replay_file(
        &replay_path,
        &project_path,
        false,
    )?;

    assert_eq!(imported.identity().devices, direct_nes_zapper_tas_devices());
    assert!(imported.branches()[0].input_at(0).zapper.trigger);
    DirectNesTasExecutionLoader::validate_project_branch_scope(&imported, "main")?;
    let mut engine = loader.load_editor_engine(&imported)?;
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &project_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let seek_cache = crate::tas_project::TasSeekStateCache::open(directory.path().join("seek"))?;
    let mut session = TasEditorSession::new(imported, &project_path, autosaves, seek_cache)?;
    engine.seek(&mut session, 1)?;
    assert_eq!(
        engine
            .backend()
            .nes_has_standard_or_zapper_controller_topology(),
        Some(true)
    );
    Ok(())
}

#[test]
fn replay_import_rejects_the_wrong_game_without_touching_the_destination() -> Result<()> {
    use zeff_emu_common::replay::{ReplayJoypadFrame, ReplayMetadata, ReplayRecorder};

    let directory = crate::test_support::test_directory("tas-loader-import-mismatch")?;
    let source_path = directory.path().join("game.nes");
    std::fs::write(&source_path, synthetic_nes_rom())?;
    let loader = DirectNesTasExecutionLoader::new(source_path, Vec::new());
    let baseline = loader.create_project()?;
    let replay_path = directory.path().join("wrong.zrpl");
    let project_path = directory.path().join("occupied.ztas");
    let metadata = ReplayMetadata {
        system: Some(baseline.identity().system.clone()),
        core_family: Some(baseline.identity().core_family.clone()),
        rom_sha256: Some([0xA5; 32]),
        ..ReplayMetadata::default()
    };
    let mut recorder = ReplayRecorder::new_with_metadata(
        PathBuf::new(),
        baseline.start_state().to_vec(),
        metadata,
    );
    recorder.record_joypad_frame(ReplayJoypadFrame::default());
    std::fs::write(&replay_path, recorder.into_bytes()?)?;
    std::fs::write(&project_path, b"occupied")?;

    let before = std::fs::read(&project_path)?;
    assert!(
        PrivateTasExecutionLoader::DirectNes(loader)
            .import_replay_file(&replay_path, &project_path, true)
            .is_err()
    );
    assert_eq!(std::fs::read(&project_path)?, before);
    Ok(())
}

#[test]
fn editor_export_verifies_saves_then_publishes_the_replay() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-loader-editor-export")?;
    let source_path = directory.path().join("game.nes");
    std::fs::write(&source_path, synthetic_nes_rom())?;
    let loader = DirectNesTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    let project_path = directory.path().join("movie.ztas");
    let replay_path = directory.path().join("movie.zrpl");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &project_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let seek_cache = crate::tas_project::TasSeekStateCache::open(directory.path().join("seek"))?;
    let mut session = TasEditorSession::new(project, &project_path, autosaves, seek_cache)?;

    PrivateTasExecutionLoader::DirectNes(loader)
        .verify_and_export_editor_session(&mut session, &replay_path)?;

    assert!(!session.is_dirty());
    assert!(replay_path.is_file());
    let saved = TasProject::load(&project_path)?;
    assert!(saved.verification_is_current("main")?);
    assert!(saved.branches()[0].verification().is_some());
    Ok(())
}

#[test]
fn canceled_editor_export_does_not_save_or_publish() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-loader-canceled-editor-export")?;
    let source_path = directory.path().join("game.nes");
    std::fs::write(&source_path, synthetic_nes_rom())?;
    let loader = DirectNesTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    let project_path = directory.path().join("movie.ztas");
    let replay_path = directory.path().join("movie.zrpl");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &project_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let seek_cache = crate::tas_project::TasSeekStateCache::open(directory.path().join("seek"))?;
    let mut session = TasEditorSession::new(project, &project_path, autosaves, seek_cache)?;
    let cancellation = AtomicBool::new(true);

    let result = PrivateTasExecutionLoader::DirectNes(loader)
        .verify_and_export_editor_session_cancellable(
            &mut session,
            &replay_path,
            &cancellation,
            &mut |_| {},
        );

    assert!(result.is_err());
    assert!(!project_path.exists());
    assert!(!replay_path.exists());
    assert!(session.selected_branch().verification().is_none());
    Ok(())
}

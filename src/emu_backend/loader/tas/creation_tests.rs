use super::*;

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

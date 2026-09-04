use std::fs;

use super::*;
use crate::emu_backend::load_backend_from_rom_source;
use crate::test_support::write_zip;

fn write_rom(path: &Path) -> Vec<u8> {
    let mut raw = vec![0; PCEAS_HEADER_LEN];
    raw[0] = 1;
    raw.extend(vec![0xEA; 0x2000]);
    fs::write(path, &raw).unwrap();
    raw
}

fn write_sf2_rom(path: &Path) -> Vec<u8> {
    let mut raw = vec![0xEA; zeff_pce_core::hardware::SF2_CE_HUCARD_IMAGE_LEN];
    raw[..6].copy_from_slice(&[0xA9, 0x00, 0x8D, 0xF3, 0x1F, 0xEA]);
    raw[0x1FFE] = 0;
    raw[0x1FFF] = 0;
    fs::write(path, &raw).unwrap();
    raw
}

fn write_populous_rom(path: &Path) -> Vec<u8> {
    let mut raw = vec![0xEA; zeff_pce_core::hardware::POPULOUS_HUCARD_IMAGE_LEN];
    raw[..12].copy_from_slice(&[
        0xA9, 0x40, 0x53, 0x02, 0xA9, 0xA5, 0x8D, 0x00, 0x20, 0x4C, 0x09, 0x00,
    ]);
    raw[0x1FFE] = 0;
    raw[0x1FFF] = 0;
    assert_eq!(
        zeff_firmware::sha256_bytes(&raw),
        TEST_POPULOUS_HUCARD_SHA256
    );
    fs::write(path, &raw).unwrap();
    raw
}

fn write_supergrafx_rom(path: &Path) -> Vec<u8> {
    let mut raw = vec![0xEA; 0x2000];
    raw[0] = 0x42;
    raw[0x1FFE] = 0;
    raw[0x1FFF] = 0;
    assert_eq!(
        zeff_firmware::sha256_bytes(&raw),
        TEST_SUPERGRAFX_HUCARD_SHA256
    );
    fs::write(path, &raw).unwrap();
    raw
}

fn base_profile(board: PceHuCardBoard) -> PceTasHardwareProfile {
    PceTasHardwareProfile {
        board,
        topology: PceHardwareTopology::Base,
        controller_mode: PceControllerMode::TwoButton,
    }
}

fn supergrafx_profile() -> PceTasHardwareProfile {
    PceTasHardwareProfile {
        board: PceHuCardBoard::Plain,
        topology: PceHardwareTopology::SuperGrafx,
        controller_mode: PceControllerMode::TwoButton,
    }
}

#[test]
fn create_open_and_replace_preserve_direct_hucard_identity() {
    let dir = crate::test_support::test_directory("pce-tas-loader").unwrap();
    let rom_path = dir.path().join("synthetic.pce");
    let raw = write_rom(&rom_path);
    let loader = DirectPceTasExecutionLoader::new(rom_path);
    let project_path = dir.path().join("movie.ztas");

    let created = loader.create_project_file(&project_path).unwrap();
    assert_eq!(
        created.identity().source_media_sha256,
        crate::tas_project::TasDigest::from_bytes(&raw)
    );
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&created).unwrap(),
        crate::emu_thread::TasExecutionProfile::DirectPceHuCard
    );
    loader.load_editor_engine(&created).unwrap();

    let replaced = loader.replace_project_file(&project_path).unwrap();
    assert_eq!(replaced.identity(), created.identity());
}

#[test]
fn six_button_direct_and_zip_projects_bind_a_distinct_controller_identity() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-six-button-loader")?;
    let rom_path = dir.path().join("six-button.pce");
    let rom = write_rom(&rom_path);
    let loader = DirectPceTasExecutionLoader::new_six_button(rom_path.clone());
    let project = loader.create_project()?;
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&project)?,
        crate::emu_thread::TasExecutionProfile::DirectPceSixButtonHuCard
    );
    assert_eq!(
        project.identity().devices[0].device,
        "pce-six-button-controller"
    );
    assert!(
        DirectPceTasExecutionLoader::new(rom_path)
            .load_editor_engine(&project)
            .is_err()
    );

    let archive_path = dir.path().join("six-button.zip");
    write_zip(&archive_path, &[("folder/six-button.pce", &rom)])?;
    let zip_loader = DirectPceTasExecutionLoader::new_zip_six_button(
        archive_path.clone(),
        Some(archive_path.join("folder/six-button.pce")),
    );
    let zip_project = zip_loader.create_project()?;
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&zip_project)?,
        crate::emu_thread::TasExecutionProfile::DirectPceSixButtonHuCard
    );
    let reopened = DirectPceTasExecutionLoader::new_zip_for_project(archive_path, &zip_project)?;
    assert_eq!(
        reopened.load_session(zip_project.start_state())?.identity(),
        zip_project.identity()
    );
    Ok(())
}

#[test]
fn loader_rejects_wrong_extension() {
    let dir = crate::test_support::test_directory("pce-tas-loader-reject").unwrap();
    let wrong = dir.path().join("synthetic.bin");
    write_rom(&wrong);
    assert!(
        DirectPceTasExecutionLoader::new(wrong)
            .load_fresh_backend()
            .is_err()
    );
}

#[test]
fn noncanonical_512k_hucard_does_not_gain_the_populous_board() {
    let mut rom = vec![0xEA; zeff_pce_core::hardware::POPULOUS_HUCARD_IMAGE_LEN];
    rom[0] = 0xA9;
    rom[0x1FFE] = 0;
    rom[0x1FFF] = 0;
    assert_eq!(
        classify_direct_pce_tas_board(&rom).unwrap(),
        PceHuCardBoard::Plain
    );
}

#[test]
fn noncanonical_hucard_does_not_gain_the_supergrafx_topology() {
    let mut rom = vec![0xEA; 0x2000];
    rom[0] = 0x43;
    rom[0x1FFE] = 0;
    rom[0x1FFF] = 0;
    assert_eq!(
        classify_direct_pce_tas_hardware(&rom).unwrap(),
        base_profile(PceHuCardBoard::Plain)
    );
}

#[test]
fn create_open_and_replace_preserve_sf2_hucard_identity() {
    let dir = crate::test_support::test_directory("pce-tas-sf2-loader").unwrap();
    let rom_path = dir.path().join("sf2.pce");
    let raw = write_sf2_rom(&rom_path);
    let loader = DirectPceTasExecutionLoader::new(rom_path);
    let project_path = dir.path().join("sf2.ztas");

    let created = loader.create_project_file(&project_path).unwrap();
    assert_eq!(
        created.identity().source_media_sha256,
        crate::tas_project::TasDigest::from_bytes(&raw)
    );
    assert_ne!(
        created.identity().sync_config_sha256,
        direct_pce_tas_sync_config_sha256_for_board(PceHuCardBoard::Plain)
    );
    assert_eq!(
        direct_pce_tas_project_board(&created).unwrap(),
        PceHuCardBoard::Sf2Ce
    );
    loader.load_editor_engine(&created).unwrap();

    let replaced = loader.replace_project_file(&project_path).unwrap();
    assert_eq!(replaced.identity(), created.identity());
}

#[test]
fn regular_direct_and_zip_loads_report_the_sf2_tas_profile() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-sf2-provenance")?;
    let rom_path = dir.path().join("sf2.pce");
    let rom = write_sf2_rom(&rom_path);
    let config = pce_tas_load_config(base_profile(PceHuCardBoard::Sf2Ce));
    let direct = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &rom_path,
        &rom_path,
        None,
        config.clone(),
    )?
    .backend;
    assert_eq!(
        direct
            .pce()
            .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
            .unwrap()
            .load
            .tas_sync_config_sha256,
        direct_pce_tas_sync_config_sha256_for_board(PceHuCardBoard::Sf2Ce).0
    );
    validate_direct_pce_tas_runtime(&direct, false)?;

    let archive_path = dir.path().join("games.zip");
    write_zip(&archive_path, &[("folder/sf2.pce", &rom)])?;
    let member_path = archive_path.join("folder/sf2.pce");
    let zipped = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &archive_path,
        &member_path,
        Some(rom),
        config,
    )?
    .backend;
    assert_eq!(
        zipped
            .pce()
            .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
            .unwrap()
            .load
            .tas_sync_config_sha256,
        zip_pce_tas_sync_config_sha256_for_board(PceHuCardBoard::Sf2Ce, "folder/sf2.pce").0
    );
    validate_direct_pce_tas_runtime(&zipped, false).map(|_| ())
}

#[test]
fn populous_direct_create_reopen_execute_and_continue_without_host_persistence() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-populous-direct")?;
    let rom_path = dir.path().join("populous.pce");
    let raw = write_populous_rom(&rom_path);
    let loader = DirectPceTasExecutionLoader::new(rom_path);
    let project_path = dir.path().join("populous.ztas");
    let mut project = loader.create_project_file(&project_path)?;
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&raw)
    );
    assert_eq!(
        direct_pce_tas_project_board(&project)?,
        PceHuCardBoard::Populous
    );
    assert_eq!(
        project.identity().persistent_state,
        crate::tas_project::TasExternalIdentity::Absent
    );
    let reopened = TasProject::load(&project_path)?;
    assert_eq!(reopened, project);

    let mut input = crate::tas_project::TasInputFrame::default();
    input.players[0].buttons = 0x01;
    input.players[0].dpad = 0x04;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let mut engine = loader.load_editor_engine(&project)?;
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &project_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let cache = crate::tas_project::TasSeekStateCache::open(dir.path().join("seek-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, &project_path, autosaves, cache)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());

    let (mut expected, _) = loader.load_fresh_backend()?;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    assert_eq!(
        engine.backend().save_ram_kind(),
        zeff_emu_common::save_ram::SaveRamKind::mapper_ram_unknown(
            zeff_pce_core::hardware::POPULOUS_HUCARD_RAM_LEN,
        )
    );
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );
    let mut actual = engine.into_backend();
    actual.step_frame();
    expected.step_frame();
    assert_eq!(actual.encode_state_bytes()?, expected.encode_state_bytes()?);
    assert_eq!(actual.flush_battery_sram()?, None);
    Ok(())
}

#[test]
fn regular_direct_and_zip_loads_report_the_populous_native_ram_profile() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-populous-provenance")?;
    let rom_path = dir.path().join("populous.pce");
    let rom = write_populous_rom(&rom_path);
    let config = pce_tas_load_config(base_profile(PceHuCardBoard::Populous));
    let direct = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &rom_path,
        &rom_path,
        None,
        config.clone(),
    )?
    .backend;
    let direct_provenance = direct
        .pce()
        .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
        .unwrap()
        .load;
    assert_eq!(
        direct_provenance.tas_sync_config_sha256,
        direct_pce_tas_sync_config_sha256_for_board(PceHuCardBoard::Populous).0
    );
    assert_eq!(
        direct_provenance.persistent_load,
        crate::emu_backend::pce::PceTasPersistentLoadOutcome::Skipped
    );
    validate_direct_pce_tas_runtime(&direct, false)?;

    let archive_path = dir.path().join("games.zip");
    write_zip(&archive_path, &[("folder/populous.pce", &rom)])?;
    let member_path = archive_path.join("folder/populous.pce");
    let zipped = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &archive_path,
        &member_path,
        Some(rom),
        config,
    )?
    .backend;
    assert_eq!(
        zipped
            .pce()
            .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
            .unwrap()
            .load
            .tas_sync_config_sha256,
        zip_pce_tas_sync_config_sha256_for_board(PceHuCardBoard::Populous, "folder/populous.pce",)
            .0
    );
    validate_direct_pce_tas_runtime(&zipped, false).map(|_| ())
}

#[test]
fn selected_zip_populous_creates_and_reopens_with_exact_board_identity() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-populous-zip")?;
    let rom_path = dir.path().join("populous.pce");
    let rom = write_populous_rom(&rom_path);
    let archive_path = dir.path().join("games.zip");
    let archive_bytes = write_zip(&archive_path, &[("folder/populous.pce", &rom)])?;
    let loader = DirectPceTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/populous.pce")),
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&archive_bytes)
    );
    assert_eq!(
        direct_pce_tas_project_board(&project)?,
        PceHuCardBoard::Populous
    );
    let reopened = DirectPceTasExecutionLoader::new_zip_for_project(archive_path, &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );
    Ok(())
}

#[test]
fn supergrafx_direct_create_reopen_execute_and_continue() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-supergrafx-direct")?;
    let rom_path = dir.path().join("supergrafx.pce");
    let raw = write_supergrafx_rom(&rom_path);
    let loader = DirectPceTasExecutionLoader::new(rom_path);
    let project_path = dir.path().join("supergrafx.ztas");
    let mut project = loader.create_project_file(&project_path)?;
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&raw)
    );
    assert_eq!(
        direct_pce_tas_project_profile(&project)?,
        supergrafx_profile()
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        direct_pce_tas_sync_config_sha256_for_profile(supergrafx_profile())
    );
    assert_eq!(
        project.identity().persistent_state,
        crate::tas_project::TasExternalIdentity::Absent
    );
    assert_eq!(TasProject::load(&project_path)?, project);

    let mut input = crate::tas_project::TasInputFrame::default();
    input.players[0].buttons = 0x02;
    input.players[0].dpad = 0x08;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let mut engine = loader.load_editor_engine(&project)?;
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &project_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let cache = crate::tas_project::TasSeekStateCache::open(dir.path().join("seek-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, &project_path, autosaves, cache)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());

    let (mut expected, _) = loader.load_fresh_backend()?;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    assert_eq!(
        engine.backend().save_ram_kind(),
        zeff_emu_common::save_ram::SaveRamKind::None
    );
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );
    let mut actual = engine.into_backend();
    actual.step_frame();
    expected.step_frame();
    assert_eq!(actual.encode_state_bytes()?, expected.encode_state_bytes()?);
    assert_eq!(actual.flush_battery_sram()?, None);
    Ok(())
}

#[test]
fn regular_direct_and_zip_loads_report_the_supergrafx_profile() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-supergrafx-provenance")?;
    let rom_path = dir.path().join("supergrafx.pce");
    let rom = write_supergrafx_rom(&rom_path);
    let profile = supergrafx_profile();
    let config = pce_tas_load_config(profile);
    let direct = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &rom_path,
        &rom_path,
        None,
        config.clone(),
    )?
    .backend;
    let direct_provenance = direct
        .pce()
        .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
        .unwrap()
        .load;
    assert_eq!(
        direct_provenance.selected_hardware,
        Some(PceCartridgeHardware::SuperGrafx)
    );
    assert_eq!(
        direct_provenance.effective_topology,
        PceHardwareTopology::SuperGrafx
    );
    assert_eq!(
        direct_provenance.tas_sync_config_sha256,
        direct_pce_tas_sync_config_sha256_for_profile(profile).0
    );
    assert_eq!(
        direct_provenance.persistent_load,
        crate::emu_backend::pce::PceTasPersistentLoadOutcome::Skipped
    );
    validate_direct_pce_tas_runtime(&direct, false)?;

    let archive_path = dir.path().join("games.zip");
    write_zip(&archive_path, &[("folder/supergrafx.pce", &rom)])?;
    let member_path = archive_path.join("folder/supergrafx.pce");
    let zipped = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &archive_path,
        &member_path,
        Some(rom),
        config,
    )?
    .backend;
    let zipped_provenance = zipped
        .pce()
        .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
        .unwrap()
        .load;
    assert_eq!(
        zipped_provenance.selected_hardware,
        Some(PceCartridgeHardware::SuperGrafx)
    );
    assert_eq!(
        zipped_provenance.effective_topology,
        PceHardwareTopology::SuperGrafx
    );
    assert_eq!(
        zipped_provenance.tas_sync_config_sha256,
        zip_pce_tas_sync_config_sha256_for_profile(profile, "folder/supergrafx.pce").0
    );
    validate_direct_pce_tas_runtime(&zipped, false).map(|_| ())
}

#[test]
fn selected_zip_supergrafx_creates_and_reopens_with_exact_topology() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-supergrafx-zip")?;
    let rom_path = dir.path().join("supergrafx.pce");
    let rom = write_supergrafx_rom(&rom_path);
    let archive_path = dir.path().join("games.zip");
    let archive_bytes = write_zip(&archive_path, &[("folder/supergrafx.pce", &rom)])?;
    let loader = DirectPceTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/supergrafx.pce")),
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&archive_bytes)
    );
    assert_eq!(
        direct_pce_tas_project_profile(&project)?,
        supergrafx_profile()
    );
    let reopened = DirectPceTasExecutionLoader::new_zip_for_project(archive_path, &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );
    Ok(())
}

#[test]
fn replay_export_and_import_preserve_direct_pce_input() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-loader-replay").unwrap();
    let rom_path = dir.path().join("synthetic.pce");
    write_rom(&rom_path);
    let loader = DirectPceTasExecutionLoader::new(rom_path);
    let mut project = loader.create_project()?;
    let mut input = crate::tas_project::TasInputFrame::default();
    input.players[0].buttons = 0x03;
    input.players[0].dpad = 0x04;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;

    let manual_path = dir.path().join("source.ztas");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &manual_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let cache = crate::tas_project::TasSeekStateCache::open(dir.path().join("replay-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let plan = super::super::PrivateTasExecutionLoader::DirectPce(loader);
    let replay_path = dir.path().join("movie.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;

    let imported_path = dir.path().join("imported.ztas");
    let imported = plan.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectPceHuCard
    );
    Ok(())
}

#[test]
fn replay_export_and_import_preserve_direct_pce_six_button_input() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-six-button-replay").unwrap();
    let rom_path = dir.path().join("synthetic.pce");
    write_rom(&rom_path);
    let loader = DirectPceTasExecutionLoader::new_six_button(rom_path);
    let mut project = loader.create_project()?;
    let mut input = crate::tas_project::TasInputFrame::default();
    input.players[0].buttons = 0x93;
    input.players[0].dpad = 0x04;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;

    let manual_path = dir.path().join("source.ztas");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &manual_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let cache = crate::tas_project::TasSeekStateCache::open(dir.path().join("replay-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let plan = super::super::PrivateTasExecutionLoader::DirectPce(loader);
    let replay_path = dir.path().join("movie.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;

    let imported_path = dir.path().join("imported.ztas");
    let imported = plan.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectPceSixButtonHuCard
    );
    Ok(())
}

#[test]
fn selected_zip_hucard_creates_reopens_and_rejects_archive_changes() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-zip").unwrap();
    let first_path = dir.path().join("first.pce");
    let selected_path = dir.path().join("selected.pce");
    let first = write_rom(&first_path);
    let mut selected = write_rom(&selected_path);
    *selected.last_mut().unwrap() ^= 1;
    let archive_path = dir.path().join("games.zip");
    let archive_bytes = write_zip(
        &archive_path,
        &[("first.pce", &first), ("folder/selected.pce", &selected)],
    )?;
    let loader = DirectPceTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/selected.pce")),
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&archive_bytes)
    );
    assert_eq!(
        project.identity().effective_media_sha256,
        TasDigest::from_bytes(&normalize_hucard_image(selected.clone())?)
    );
    let reopened =
        DirectPceTasExecutionLoader::new_zip_for_project(archive_path.clone(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );

    write_zip(
        &archive_path,
        &[
            ("first.pce", &first),
            ("folder/selected.pce", &selected),
            ("note.txt", b"changed"),
        ],
    )?;
    assert!(DirectPceTasExecutionLoader::new_zip_for_project(archive_path, &project).is_err());
    Ok(())
}

#[test]
fn zip_hucard_keeps_board_and_selection_gates() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-zip-gates").unwrap();
    let archive_path = dir.path().join("games.zip");
    let sf2_path = dir.path().join("sf2.pce");
    let sf2 = write_sf2_rom(&sf2_path);
    write_zip(&archive_path, &[("sf2.pce", &sf2)])?;
    let project =
        DirectPceTasExecutionLoader::new_zip(archive_path.clone(), None).create_project()?;
    assert_eq!(
        direct_pce_tas_project_board(&project)?,
        PceHuCardBoard::Sf2Ce
    );
    let reopened =
        DirectPceTasExecutionLoader::new_zip_for_project(archive_path.clone(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );

    let rom_path = dir.path().join("plain.pce");
    let plain = write_rom(&rom_path);
    write_zip(&archive_path, &[("one.pce", &plain), ("two.pce", &plain)])?;
    assert!(
        DirectPceTasExecutionLoader::new_zip(archive_path, None)
            .create_project()
            .is_err()
    );
    Ok(())
}

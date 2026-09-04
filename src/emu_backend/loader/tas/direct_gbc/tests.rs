use anyhow::Result;
use zeff_gb_core::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};

use super::super::PrivateTasExecutionLoader;
use super::*;
use crate::emu_backend::ActiveSystem;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasEditorSession, TasExecutionWitness,
    TasInputFrame, TasSeekStateCache,
};
use crate::test_support::{build_gb_test_rom, test_directory, write_zip};

fn cgb_rom() -> Vec<u8> {
    let mut rom = build_gb_test_rom();
    rom[0x143] = 0xC0;
    rom
}

fn cgb_mapper_rom(cartridge_type: u8, rom_size: u8, ram_size: u8) -> Vec<u8> {
    let mut rom = cgb_rom();
    rom.resize((32 * 1024usize) << rom_size, 0);
    rom[0x147] = cartridge_type;
    rom[0x148] = rom_size;
    rom[0x149] = ram_size;
    rom
}

fn mbc3_rtc_sidecar(ram_len: usize, saved_seconds: u64) -> (Vec<u8>, Vec<u8>) {
    let ram = (0..ram_len)
        .map(|index| (index as u8).wrapping_mul(23).wrapping_add(9))
        .collect::<Vec<_>>();
    let mut bytes = ram.clone();
    for register in [12u8, 24, 6, 7, 0, 13, 25, 6, 7, 0] {
        bytes.extend_from_slice(&(register as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&saved_seconds.to_le_bytes());
    (ram, bytes)
}

fn write_cgb_rom(label: &str) -> Result<(crate::test_support::TestDirectory, PathBuf, Vec<u8>)> {
    let directory = test_directory(label)?;
    let path = directory.path().join("game.gbc");
    let bytes = cgb_rom();
    std::fs::write(&path, &bytes)?;
    Ok((directory, path, bytes))
}

#[test]
fn creates_classifies_and_seeks_a_forced_cgb_project() -> Result<()> {
    let (directory, source_path, mut source_bytes) = write_cgb_rom("tas-direct-gbc-flow")?;
    source_bytes[0x147] = 0x08;
    source_bytes[0x149] = 0x02;
    let save_path = source_path.with_extension("sav");
    let initial_sidecar = vec![0xA5; 8 * 1024];
    std::fs::write(&source_path, &source_bytes)?;
    std::fs::write(&save_path, &initial_sidecar)?;
    let loader = DirectGbcTasExecutionLoader::new(source_path.clone(), Vec::new());
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );
    let changed_sidecar = vec![0x5A; initial_sidecar.len()];
    std::fs::write(&save_path, &changed_sidecar)?;
    let repeated = loader.create_project()?;
    assert_eq!(repeated.identity(), project.identity());
    assert_eq!(repeated.start_state(), project.start_state());

    assert_eq!(
        project.project_id(),
        format!("gbc-{}", TasDigest::from_bytes(&source_bytes).to_hex())
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        direct_gbc_tas_sync_config_sha256()
    );
    assert_eq!(
        project.identity().state_format_compatibility_id,
        zeff_gb_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
    );
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&project)?,
        crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb
    );
    let (backend, _) = loader.load_fresh_backend()?;
    let provenance = backend.gb_tas_load_provenance().unwrap();
    assert!(provenance.load.direct_gbc_file);
    assert!(!provenance.load.direct_gb_file);
    assert_eq!(
        provenance.current_hardware_mode_preference,
        HardwareModePreference::ForceCgb
    );
    assert_eq!(provenance.current_hardware_mode, HardwareMode::CGBNormal);

    let mut engine = loader.load_editor_engine(&project)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;
    let outcome = engine.seek(&mut editor, 1)?;
    assert!(outcome.reached_target());
    assert_eq!(outcome.cursor, 1);
    assert_eq!(std::fs::read(save_path)?, changed_sidecar);
    Ok(())
}

#[test]
fn selector_routes_only_direct_gbc_media_to_the_cgb_loader() -> Result<()> {
    let (_directory, source_path, _) = write_cgb_rom("tas-direct-gbc-selector")?;
    assert!(matches!(
        super::super::select_private_tas_execution_loader(
            source_path,
            ActiveSystem::GameBoy,
            Vec::new()
        )?,
        super::super::PrivateTasExecutionLoader::DirectGbc(_)
    ));
    Ok(())
}

#[test]
fn zip_member_binds_cgb_profile_and_rejects_archive_mutation() -> Result<()> {
    let directory = test_directory("tas-gbc-zip")?;
    let archive_path = directory.path().join("games.zip");
    let first = cgb_rom();
    let selected = cgb_mapper_rom(0x08, 0x00, 0x02);
    let archive_bytes = write_zip(
        &archive_path,
        &[("first.gbc", &first), ("folder/game.gbc", &selected)],
    )?;
    let save_path = archive_path.with_extension("sav");
    let initial_sidecar = vec![0xA5; 8 * 1024];
    std::fs::write(&save_path, &initial_sidecar)?;
    let loader = DirectGbcTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/game.gbc")),
        Vec::new(),
    );
    let project = loader.create_project()?;
    let changed_sidecar = vec![0x3C; 8 * 1024];
    std::fs::write(&save_path, &changed_sidecar)?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );
    assert_eq!(loader.create_project()?.identity(), project.identity());
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
        zip_gbc_tas_sync_config_sha256("folder/game.gbc")
    );
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&project)?,
        crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb
    );
    let reopened = DirectGbcTasExecutionLoader::new_zip_for_project(
        archive_path.clone(),
        Vec::new(),
        &project,
    )?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );
    let mut engine = reopened.load_editor_engine(&project)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project.clone(), manual_path, autosaves, seek_cache)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    assert_eq!(std::fs::read(&save_path)?, changed_sidecar);
    let (backend, _) = reopened.load_fresh_backend()?;
    let witness = crate::emu_thread::build_tas_repair_witness(
        &backend,
        crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb,
    )
    .expect("GBC ZIP backend should produce a TAS witness");
    assert_eq!(
        witness.source_media_sha256,
        TasDigest::from_bytes(&archive_bytes)
    );
    assert_eq!(
        witness.effective_media_sha256,
        TasDigest::from_bytes(&selected)
    );
    assert_eq!(
        witness.sync_config_sha256,
        zip_gbc_tas_sync_config_sha256("folder/game.gbc")
    );

    write_zip(
        &archive_path,
        &[
            ("first.gbc", &first),
            ("folder/game.gbc", &selected),
            ("note.txt", b"mutation"),
        ],
    )?;
    assert!(
        DirectGbcTasExecutionLoader::new_zip_for_project(archive_path, Vec::new(), &project)
            .is_err()
    );
    Ok(())
}

#[test]
fn zip_cgb_rejects_ambiguity_and_battery_sidecars() -> Result<()> {
    let directory = test_directory("tas-gbc-zip-gates")?;
    let archive_path = directory.path().join("games.zip");
    let plain = cgb_rom();
    write_zip(&archive_path, &[("one.gbc", &plain), ("two.gbc", &plain)])?;
    assert!(
        DirectGbcTasExecutionLoader::new_zip(archive_path.clone(), None, Vec::new())
            .create_project()
            .is_err()
    );

    let battery = cgb_mapper_rom(0x03, 0x04, 0x03);
    let sidecar = vec![0xC3; 32 * 1024];
    write_zip(
        &archive_path,
        &[("folder/game.gbc", &battery), ("folder/game.sav", &sidecar)],
    )?;
    assert!(
        DirectGbcTasExecutionLoader::new_zip(
            archive_path.clone(),
            Some(archive_path.join("folder/game.gbc")),
            Vec::new(),
        )
        .create_project()
        .is_err()
    );
    Ok(())
}

#[test]
fn zip_cgb_battery_project_imports_adjacent_sram_once() -> Result<()> {
    let directory = test_directory("tas-gbc-zip-battery")?;
    let archive_path = directory.path().join("games.zip");
    let save_path = archive_path.with_extension("sav");
    let battery = cgb_mapper_rom(0x09, 0x00, 0x02);
    let initial_sram = (0..8 * 1024)
        .map(|index| (index as u8).wrapping_mul(19).wrapping_add(5))
        .collect::<Vec<_>>();
    write_zip(&archive_path, &[("folder/game.gbc", &battery)])?;
    std::fs::write(&save_path, &initial_sram)?;

    let loader = DirectGbcTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/game.gbc")),
        Vec::new(),
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial_sram))
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        zip_gbc_battery_tas_sync_config_sha256("folder/game.gbc")
    );

    let changed_sidecar = vec![0xD1; initial_sram.len()];
    std::fs::write(&save_path, &changed_sidecar)?;
    let reopened =
        DirectGbcTasExecutionLoader::new_zip_for_project(archive_path, Vec::new(), &project)?;
    let mut engine = reopened.load_editor_engine(&project)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    assert_eq!(std::fs::read(save_path)?, changed_sidecar);
    Ok(())
}

#[test]
fn battery_project_owns_initial_sram_and_ignores_later_sidecar_changes() -> Result<()> {
    let directory = test_directory("tas-direct-gbc-battery")?;
    let source_path = directory.path().join("game.gbc");
    let save_path = directory.path().join("game.sav");
    let source = cgb_mapper_rom(0x09, 0x00, 0x02);
    let initial_sram = (0..8 * 1024)
        .map(|index| (index as u8).wrapping_mul(31).wrapping_add(3))
        .collect::<Vec<_>>();
    std::fs::write(&source_path, source)?;
    std::fs::write(&save_path, &initial_sram)?;

    let loader = DirectGbcTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial_sram))
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        direct_gbc_battery_tas_sync_config_sha256()
    );

    let changed_sidecar = vec![0xB9; initial_sram.len()];
    std::fs::write(&save_path, &changed_sidecar)?;
    let mut engine = loader.load_editor_engine(&project)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    assert_eq!(std::fs::read(save_path)?, changed_sidecar);
    Ok(())
}

#[test]
fn direct_cgb_mbc3_rtc_is_fixed_epoch_and_headless_deterministic() -> Result<()> {
    let directory = test_directory("tas-direct-gbc-rtc")?;
    let source_path = directory.path().join("clock.gbc");
    let save_path = source_path.with_extension("sav");
    let rom = cgb_mapper_rom(0x10, 0x06, 0x03);
    let (ram, sidecar) = mbc3_rtc_sidecar(
        32 * 1024,
        super::super::gb_rtc::GB_TAS_RTC_EPOCH_UNIX_SECONDS - 7,
    );
    std::fs::write(&source_path, rom)?;
    std::fs::write(&save_path, &sidecar)?;
    let loader = DirectGbcTasExecutionLoader::new(source_path, Vec::new());

    let project = loader.create_project()?;
    let repeated = loader.create_project()?;
    let mut canonical_start = project.start_state().to_vec();
    zeff_gb_core::save_state::canonicalize_bess_rtc_timestamp(&mut canonical_start);
    assert_eq!(project.start_state(), canonical_start);
    assert_eq!(project.identity(), repeated.identity());
    assert_eq!(project.start_state(), repeated.start_state());
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&ram))
    );
    assert!(matches!(
        project.identity().rtc_state,
        TasExternalIdentity::ExternalSha256(_)
    ));
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::gb_rtc::gb_rtc_sync_config_sha256(
            super::super::gb_rtc::GbTasRtcHardware::Cgb,
            32 * 1024,
            None,
        )
    );
    let linked_candidate = loader.load_editor_engine(&project)?;
    let persistence_witness =
        super::super::gb_rtc::gb_rtc_persistence_witness(linked_candidate.backend())?;
    assert_eq!(
        persistence_witness.persistent_state,
        project.identity().persistent_state
    );
    assert_eq!(persistence_witness.rtc_state, project.identity().rtc_state);
    assert_eq!(
        persistence_witness.complete_byte_len,
        (32 * 1024 + 64) as u64
    );
    let linked_state = linked_candidate.backend().encode_state_bytes()?;
    assert!(
        validate_direct_gbc_tas_runtime_with_project_sram(linked_candidate.backend(), false)
            .is_err()
    );
    assert!(
        validate_direct_gbc_state_for_backend_with_project_sram(
            linked_candidate.backend(),
            &linked_state,
            true,
        )
        .is_err()
    );

    std::fs::write(&save_path, vec![0xD7; sidecar.len()])?;
    let plan = PrivateTasExecutionLoader::DirectGbc(loader);
    let start_state = project.start_state().to_vec();
    let witness_session = plan.load_session(&start_state)?;
    let witness = TasExecutionWitness {
        identity: witness_session.identity().clone(),
    };
    let mut verified = project;
    verified.verify_branch_with_factory("main", &witness, || plan.load_session(&start_state))?;
    assert_eq!(std::fs::read(save_path)?, vec![0xD7; sidecar.len()]);
    Ok(())
}

#[test]
fn selected_zip_cgb_mbc3_rtc_binds_member_and_outer_sidecar() -> Result<()> {
    let directory = test_directory("tas-zip-gbc-rtc")?;
    let archive_path = directory.path().join("clocks.zip");
    let save_path = archive_path.with_extension("sav");
    let rom = cgb_mapper_rom(0x10, 0x06, 0x03);
    let (ram, sidecar) = mbc3_rtc_sidecar(
        32 * 1024,
        super::super::gb_rtc::GB_TAS_RTC_EPOCH_UNIX_SECONDS - 11,
    );
    let archive = write_zip(&archive_path, &[("folder/clock.gbc", &rom)])?;
    std::fs::write(&save_path, &sidecar)?;
    let loader = DirectGbcTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/clock.gbc")),
        Vec::new(),
    );

    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&archive)
    );
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&ram))
    );
    assert!(matches!(
        project.identity().rtc_state,
        TasExternalIdentity::ExternalSha256(_)
    ));
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::gb_rtc::gb_rtc_sync_config_sha256(
            super::super::gb_rtc::GbTasRtcHardware::Cgb,
            32 * 1024,
            Some("folder/clock.gbc"),
        )
    );
    std::fs::write(&save_path, vec![0xE1; sidecar.len()])?;
    let reopened =
        DirectGbcTasExecutionLoader::new_zip_for_project(archive_path, Vec::new(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );
    assert_eq!(std::fs::read(save_path)?, vec![0xE1; sidecar.len()]);
    Ok(())
}

#[test]
fn accepts_plain_battery_mappers_without_rtc_or_devices() {
    for (label, cartridge_type, rom_size, ram_size) in [
        ("mbc1", 0x03, 0x04, 0x03),
        ("mbc2", 0x06, 0x03, 0x00),
        ("mbc3-no-rtc", 0x13, 0x06, 0x03),
        ("mbc5", 0x1B, 0x08, 0x04),
    ] {
        assert!(
            validate_direct_gbc_rom(&cgb_mapper_rom(cartridge_type, rom_size, ram_size)).is_ok(),
            "{label}"
        );
    }
}

#[test]
fn rejects_color_compatible_dmg_and_external_state_media() -> Result<()> {
    let (directory, source_path, _) = write_cgb_rom("tas-direct-gbc-rejections")?;
    for (cgb_flag, cartridge_type, ram_size) in [
        (0x80, 0x00, 0x00),
        (0x00, 0x00, 0x00),
        (0xC0, 0x08, 0x00),
        (0xC0, 0x09, 0x00),
        (0xC0, 0x1E, 0x03),
        (0xC0, 0x22, 0x02),
        (0xC0, 0xFC, 0x03),
        (0xC0, 0xFE, 0x03),
    ] {
        let mut rom = cgb_rom();
        rom[0x143] = cgb_flag;
        rom[0x147] = cartridge_type;
        rom[0x149] = ram_size;
        std::fs::write(&source_path, rom)?;
        assert!(
            DirectGbcTasExecutionLoader::new(source_path.clone(), Vec::new())
                .create_project()
                .is_err()
        );
    }
    std::fs::write(&source_path, cgb_mapper_rom(0x09, 0x01, 0x02))?;
    assert!(
        DirectGbcTasExecutionLoader::new(source_path.clone(), Vec::new())
            .create_project()
            .is_err()
    );
    drop(directory);
    Ok(())
}

#[test]
fn changed_media_and_wrong_hardware_state_are_rejected() -> Result<()> {
    let (_directory, source_path, _) = write_cgb_rom("tas-direct-gbc-state")?;
    let loader = DirectGbcTasExecutionLoader::new(source_path.clone(), Vec::new());
    let project = loader.create_project()?;
    let (backend, _) = loader.load_fresh_backend()?;
    let dmg = zeff_gb_core::emulator::Emulator::from_rom_data(
        &cgb_rom(),
        HardwareModePreference::ForceDmg,
    )?;
    assert!(
        validate_direct_gbc_state_for_backend(&backend, &dmg.encode_state_bytes()?, true).is_err()
    );
    let mut changed = cgb_rom();
    changed[0x200] ^= 0xFF;
    std::fs::write(&source_path, changed)?;
    assert!(loader.load_session(project.start_state()).is_err());
    Ok(())
}

#[test]
fn replay_export_import_uses_two_fresh_cgb_passes() -> Result<()> {
    let (directory, source_path, mut source) = write_cgb_rom("tas-direct-gbc-replay")?;
    source[0x147] = 0x09;
    source[0x149] = 0x02;
    std::fs::write(&source_path, source)?;
    std::fs::write(source_path.with_extension("sav"), vec![0x4A; 8 * 1024])?;
    let loader = DirectGbcTasExecutionLoader::new(source_path.clone(), Vec::new());
    let mut project = loader.create_project()?;
    let input = TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 1,
                dpad: 2,
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
    let plan = super::super::select_private_tas_execution_loader_for_project(
        source_path,
        ActiveSystem::GameBoy,
        Vec::new(),
        editor.project(),
    )?;
    assert!(matches!(
        plan,
        super::super::PrivateTasExecutionLoader::DirectGbc(_)
    ));
    let replay_path = directory.path().join("movie.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;
    assert!(editor.project().verification_is_current("main")?);

    let imported_path = directory.path().join("imported.ztas");
    let imported = plan.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectGbCartridgeCgb
    );
    Ok(())
}

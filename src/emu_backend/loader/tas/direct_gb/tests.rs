use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use zeff_emu_common::replay::{ReplayEvent, ReplayStartMetadata};

use super::super::direct_gb_loader::{DirectGbTasExecutionLoader, direct_gb_tas_identity};
use super::super::{
    PrivateTasExecutionLoader, classify_direct_tas_execution_profile,
    select_private_tas_execution_loader,
};
use super::*;
use crate::emu_backend::gb::{GbPersistentLoadOutcome, GbTasLoadProvenanceSeed, GbTasLoadSetup};
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, GbBackend, load_backend_from_rom_source,
};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasEditorExecutionProvider,
    TasEditorSession, TasExecutionWitness, TasInitialBranch, TasInputFrame, TasSeekStateCache,
};
use crate::test_support::{build_gb_test_rom, test_directory, write_zip};

mod replay;

fn write_direct_rom(label: &str) -> Result<(crate::test_support::TestDirectory, PathBuf, Vec<u8>)> {
    let directory = test_directory(label)?;
    let path = directory.path().join("game.gb");
    let bytes = build_gb_test_rom();
    std::fs::write(&path, &bytes)?;
    Ok((directory, path, bytes))
}

#[test]
fn sync_configuration_digest_remains_compatible_with_existing_projects() {
    assert_eq!(
        direct_gb_tas_sync_config_sha256().to_hex(),
        "ef7149d17595ee4a0e218a6b243005c0d300fea956e5a1a39243d5086926f940"
    );
}

fn build_direct_mapper_rom(cartridge_type: u8, rom_size: u8, ram_size: u8) -> Vec<u8> {
    let len = match rom_size {
        0x00..=0x08 => (32 * 1024usize) << rom_size,
        0x52 => 72 * 16 * 1024,
        0x53 => 80 * 16 * 1024,
        0x54 => 96 * 16 * 1024,
        _ => 32 * 1024,
    };
    let mut bytes = build_gb_test_rom();
    bytes.resize(len, 0);
    bytes[0x147] = cartridge_type;
    bytes[0x148] = rom_size;
    bytes[0x149] = ram_size;
    bytes
}

fn mbc3_rtc_sidecar(ram_len: usize, saved_seconds: u64) -> (Vec<u8>, Vec<u8>) {
    let ram = (0..ram_len)
        .map(|index| (index as u8).wrapping_mul(13).wrapping_add(7))
        .collect::<Vec<_>>();
    let mut bytes = ram.clone();
    for register in [10u8, 20, 3, 4, 0, 11, 21, 3, 4, 0] {
        bytes.extend_from_slice(&(register as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&saved_seconds.to_le_bytes());
    (ram, bytes)
}

#[test]
fn creates_reopens_and_seeks_a_direct_rom_only_project() -> Result<()> {
    let (directory, source_path, source_bytes) = write_direct_rom("tas-direct-gb-flow")?;
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let project_path = directory.path().join("movie.ztas");

    let in_memory = loader.create_project()?;
    let decoded = TasProject::decode(&in_memory.encode()?)?;
    assert_eq!(decoded.project_id(), in_memory.project_id());
    assert_eq!(decoded.identity(), in_memory.identity());
    assert_eq!(decoded.start_state(), in_memory.start_state());
    assert_eq!(decoded.replay_start(), in_memory.replay_start());
    assert_eq!(decoded.branches(), in_memory.branches());
    assert_eq!(decoded.edit_generation(), in_memory.edit_generation());
    assert_eq!(decoded.rerecord_count(), in_memory.rerecord_count());
    assert_eq!(decoded.active_branch_id(), in_memory.active_branch_id());
    let created = loader.create_project_file(&project_path)?;
    let reopened = TasProject::load(&project_path)?;
    assert_eq!(created, reopened);
    assert_eq!(
        reopened.project_id(),
        format!("gb-{}", TasDigest::from_bytes(&source_bytes).to_hex())
    );
    assert_eq!(reopened.identity().devices, direct_gb_tas_devices());
    assert_eq!(
        reopened.identity().state_format_compatibility_id,
        zeff_gb_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
    );
    assert_eq!(
        reopened.identity().sync_config_sha256,
        direct_gb_tas_sync_config_sha256()
    );
    assert_eq!(reopened.branches().len(), 1);
    assert_eq!(reopened.branches()[0].frame_count(), 1);

    let mut engine = loader.load_editor_engine(&reopened)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(reopened, manual_path, autosaves, seek_cache)?;
    let outcome = engine.seek(&mut editor, 1)?;
    assert!(outcome.reached_target());
    assert_eq!(outcome.cursor, 1);
    assert_eq!(outcome.framebuffer.width(), 160);
    assert_eq!(outcome.framebuffer.height(), 144);
    Ok(())
}

#[test]
fn core_neutral_provider_selects_and_executes_the_direct_gb_profile() -> Result<()> {
    let (directory, source_path, _) = write_direct_rom("tas-direct-gb-provider")?;
    let loader =
        select_private_tas_execution_loader(source_path, ActiveSystem::GameBoy, Vec::new())?;
    let PrivateTasExecutionLoader::DirectGb(loader) = loader else {
        panic!("Game Boy source must select the direct Game Boy loader");
    };
    let project = loader.create_project()?;
    assert_eq!(
        classify_direct_tas_execution_profile(&project)?,
        crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg
    );

    let execution_loader = PrivateTasExecutionLoader::DirectGb(loader);
    let provider: &dyn TasEditorExecutionProvider = &execution_loader;
    let mut engine = provider.load_editor_engine(&project)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;
    let outcome = engine.seek(&mut editor, 1)?;

    assert!(outcome.reached_target());
    assert_eq!(
        (outcome.framebuffer.width(), outcome.framebuffer.height()),
        (160, 144)
    );
    Ok(())
}

#[test]
fn zip_member_binds_archive_member_and_effective_media() -> Result<()> {
    let directory = test_directory("tas-gb-zip")?;
    let archive_path = directory.path().join("games.zip");
    let first = build_gb_test_rom();
    let mut selected = first.clone();
    selected[0x200] = 0xA5;
    let archive_bytes = write_zip(
        &archive_path,
        &[("first.gb", &first), ("folder/selected.gb", &selected)],
    )?;
    assert!(
        DirectGbTasExecutionLoader::new_zip(archive_path.clone(), None, Vec::new(),)
            .create_project()
            .is_err()
    );
    let loader = DirectGbTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/selected.gb")),
        Vec::new(),
    );
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
        zip_gb_tas_sync_config_sha256("folder/selected.gb")
    );
    let reopened = DirectGbTasExecutionLoader::new_zip_for_project(
        archive_path.clone(),
        Vec::new(),
        &project,
    )?;
    let session = reopened.load_session(project.start_state())?;
    assert_eq!(session.identity(), project.identity());
    let (backend, _) = reopened.load_fresh_backend()?;
    assert_eq!(
        backend.tas_source_media_identity().unwrap().sha256,
        TasDigest::from_bytes(&archive_bytes).0
    );
    let witness = crate::emu_thread::build_tas_repair_witness(
        &backend,
        crate::emu_thread::TasExecutionProfile::DirectGbCartridgeDmg,
    )
    .expect("GB ZIP backend should produce a TAS witness");
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
        zip_gb_tas_sync_config_sha256("folder/selected.gb")
    );

    write_zip(
        &archive_path,
        &[
            ("first.gb", &first),
            ("folder/selected.gb", &selected),
            ("note.txt", b"mutation"),
        ],
    )?;
    assert!(
        DirectGbTasExecutionLoader::new_zip_for_project(archive_path, Vec::new(), &project)
            .is_err()
    );
    Ok(())
}

#[test]
fn zip_auto_selection_rejects_unsafe_paths_and_battery_sidecars() -> Result<()> {
    let directory = test_directory("tas-gb-zip-gates")?;
    let archive_path = directory.path().join("games.zip");
    let plain = build_gb_test_rom();
    write_zip(&archive_path, &[("only.gb", &plain)])?;
    let project = DirectGbTasExecutionLoader::new_zip(archive_path.clone(), None, Vec::new())
        .create_project()?;
    assert_eq!(
        project.identity().sync_config_sha256,
        zip_gb_tas_sync_config_sha256("only.gb")
    );

    write_zip(&archive_path, &[("../unsafe.gb", &plain)])?;
    assert!(
        DirectGbTasExecutionLoader::new_zip(archive_path.clone(), None, Vec::new())
            .create_project()
            .is_err()
    );

    let battery = build_direct_mapper_rom(0x03, 0x04, 0x03);
    let sidecar = vec![0x5A; 32 * 1024];
    write_zip(
        &archive_path,
        &[("folder/game.gb", &battery), ("folder/game.sav", &sidecar)],
    )?;
    assert!(
        DirectGbTasExecutionLoader::new_zip(
            archive_path.clone(),
            Some(archive_path.join("folder/game.gb")),
            Vec::new(),
        )
        .create_project()
        .is_err()
    );
    Ok(())
}

#[test]
fn zip_battery_project_imports_adjacent_sram_once() -> Result<()> {
    let directory = test_directory("tas-gb-zip-battery")?;
    let archive_path = directory.path().join("games.zip");
    let save_path = archive_path.with_extension("sav");
    let battery = build_direct_mapper_rom(0x03, 0x04, 0x03);
    let initial_sram = (0..32 * 1024)
        .map(|index| (index as u8).wrapping_mul(17).wrapping_add(11))
        .collect::<Vec<_>>();
    write_zip(&archive_path, &[("folder/game.gb", &battery)])?;
    std::fs::write(&save_path, &initial_sram)?;

    let loader = DirectGbTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/game.gb")),
        Vec::new(),
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial_sram))
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        zip_gb_battery_tas_sync_config_sha256("folder/game.gb")
    );

    let changed_sidecar = vec![0xE7; initial_sram.len()];
    std::fs::write(&save_path, &changed_sidecar)?;
    let reopened =
        DirectGbTasExecutionLoader::new_zip_for_project(archive_path, Vec::new(), &project)?;
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
fn direct_mbc3_rtc_project_is_fixed_epoch_and_headless_deterministic() -> Result<()> {
    let directory = test_directory("tas-direct-gb-rtc")?;
    let source_path = directory.path().join("clock.gb");
    let save_path = source_path.with_extension("sav");
    let rom = build_direct_mapper_rom(0x10, 0x06, 0x03);
    let (ram, sidecar) = mbc3_rtc_sidecar(
        32 * 1024,
        super::super::gb_rtc::GB_TAS_RTC_EPOCH_UNIX_SECONDS - 5,
    );
    std::fs::write(&source_path, rom)?;
    std::fs::write(&save_path, &sidecar)?;
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());

    let project = loader.create_project()?;
    let repeated = loader.create_project()?;
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
            super::super::gb_rtc::GbTasRtcHardware::Dmg,
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
    assert!(
        validate_direct_gb_tas_runtime_with_project_sram(linked_candidate.backend(), false)
            .is_err()
    );

    std::fs::write(&save_path, vec![0xE7; sidecar.len()])?;
    let plan = PrivateTasExecutionLoader::DirectGb(loader);
    let start_state = project.start_state().to_vec();
    let witness_session = plan.load_session(&start_state)?;
    let witness = TasExecutionWitness {
        identity: witness_session.identity().clone(),
    };
    let mut verified = project;
    verified.verify_branch_with_factory("main", &witness, || plan.load_session(&start_state))?;
    assert_eq!(std::fs::read(save_path)?, vec![0xE7; sidecar.len()]);
    Ok(())
}

#[test]
fn selected_zip_mbc3_rtc_binds_archive_member_and_outer_sidecar() -> Result<()> {
    let directory = test_directory("tas-zip-gb-rtc")?;
    let archive_path = directory.path().join("clocks.zip");
    let save_path = archive_path.with_extension("sav");
    let rom = build_direct_mapper_rom(0x10, 0x06, 0x03);
    let (ram, sidecar) = mbc3_rtc_sidecar(
        32 * 1024,
        super::super::gb_rtc::GB_TAS_RTC_EPOCH_UNIX_SECONDS - 9,
    );
    let archive = write_zip(&archive_path, &[("folder/clock.gb", &rom)])?;
    std::fs::write(&save_path, &sidecar)?;
    let loader = DirectGbTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/clock.gb")),
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
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::gb_rtc::gb_rtc_sync_config_sha256(
            super::super::gb_rtc::GbTasRtcHardware::Dmg,
            32 * 1024,
            Some("folder/clock.gb"),
        )
    );
    let reopened =
        DirectGbTasExecutionLoader::new_zip_for_project(archive_path, Vec::new(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );
    Ok(())
}

#[test]
fn creates_and_seeks_the_largest_supported_mapper_cartridge() -> Result<()> {
    let directory = test_directory("tas-direct-gb-largest-mapper")?;
    let source_path = directory.path().join("game.gb");
    std::fs::write(&source_path, build_direct_mapper_rom(0x1A, 0x08, 0x04))?;
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    let mut engine = loader.load_editor_engine(&project)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;

    let outcome = engine.seek(&mut editor, 1)?;

    assert!(outcome.reached_target());
    assert_eq!(outcome.cursor, 1);
    Ok(())
}

#[test]
fn accepts_every_supported_non_battery_cartridge_class() {
    for (label, cartridge_type, rom_size, ram_size) in [
        ("rom-only", 0x00, 0x00, 0x00),
        ("mbc1", 0x01, 0x06, 0x00),
        ("mbc1-ram", 0x02, 0x04, 0x03),
        ("mbc1-large-ram", 0x02, 0x06, 0x02),
        ("mbc2", 0x05, 0x03, 0x00),
        ("mbc3", 0x11, 0x06, 0x00),
        ("mbc3-ram", 0x12, 0x06, 0x03),
        ("mbc5", 0x19, 0x08, 0x00),
        ("mbc5-ram", 0x1A, 0x08, 0x04),
    ] {
        assert!(
            validate_direct_gb_rom(&build_direct_mapper_rom(cartridge_type, rom_size, ram_size,))
                .is_ok(),
            "{label}"
        );
    }
}

#[test]
fn accepts_supported_battery_cartridge_classes() {
    for (label, cartridge_type, rom_size, ram_size) in [
        ("mbc1", 0x03, 0x04, 0x03),
        ("mbc2", 0x06, 0x03, 0x00),
        ("mbc3-no-rtc", 0x13, 0x06, 0x03),
        ("mbc3-timer", 0x0F, 0x06, 0x00),
        ("mbc3-timer-ram", 0x10, 0x06, 0x03),
        ("mbc5", 0x1B, 0x08, 0x04),
    ] {
        assert!(
            validate_direct_gb_rom(&build_direct_mapper_rom(cartridge_type, rom_size, ram_size,))
                .is_ok(),
            "{label}"
        );
    }
}

#[test]
fn battery_project_owns_initial_sram_and_never_writes_the_sidecar() -> Result<()> {
    let directory = test_directory("tas-direct-gb-battery")?;
    let source_path = directory.path().join("game.gb");
    let save_path = directory.path().join("game.sav");
    let source = build_direct_mapper_rom(0x03, 0x04, 0x03);
    let initial_sram = (0..32 * 1024)
        .map(|index| (index as u8).wrapping_mul(29).wrapping_add(7))
        .collect::<Vec<_>>();
    std::fs::write(&source_path, source)?;
    std::fs::write(&save_path, &initial_sram)?;

    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial_sram))
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        direct_gb_battery_tas_sync_config_sha256()
    );

    let changed_sidecar = vec![0xD3; initial_sram.len()];
    std::fs::write(&save_path, &changed_sidecar)?;
    let mut wrong_identity = project.identity().clone();
    wrong_identity.persistent_state =
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(b"wrong SRAM"));
    let wrong_project = TasProject::new(
        "wrong-sram",
        wrong_identity,
        project.start_state().to_vec(),
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )?;
    assert!(loader.load_editor_engine(&wrong_project).is_err());

    let mut engine = loader.load_editor_engine(&project)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    assert_eq!(std::fs::read(&save_path)?, changed_sidecar);
    Ok(())
}

#[test]
fn rejects_ineligible_media_before_creating_a_project() -> Result<()> {
    let (directory, source_path, _) = write_direct_rom("tas-direct-gb-media")?;
    for label in ["wrong-size", "too-large", "declared-size", "ram", "cgb"] {
        let mut bytes = build_gb_test_rom();
        match label {
            "wrong-size" => {
                bytes.pop();
            }
            "too-large" => {
                bytes = build_direct_mapper_rom(0x19, 0x08, 0x00);
                bytes.push(0);
            }
            "declared-size" => bytes[0x148] = 0x01,
            "ram" => bytes[0x149] = 0x02,
            "cgb" => bytes[0x143] = 0xC0,
            _ => unreachable!(),
        }
        std::fs::write(&source_path, bytes)?;
        let error = DirectGbTasExecutionLoader::new(source_path.clone(), Vec::new())
            .create_project()
            .unwrap_err();
        assert!(!error.to_string().is_empty(), "{label}");
    }
    drop(directory);
    Ok(())
}

#[test]
fn rejects_persistent_external_hardware_and_invalid_mapper_sizes() {
    for (label, cartridge_type, rom_size, ram_size) in [
        ("rom-ram", 0x08, 0x00, 0x02),
        ("mbc5-rumble", 0x1C, 0x08, 0x00),
        ("mbc7-sensor", 0x22, 0x06, 0x02),
        ("camera", 0xFC, 0x05, 0x03),
        ("huc3", 0xFE, 0x05, 0x03),
        ("mbc1-too-large", 0x01, 0x07, 0x00),
        ("mbc1-too-much-ram", 0x02, 0x06, 0x03),
        ("mbc2-too-large", 0x05, 0x04, 0x00),
        ("mbc3-too-large", 0x11, 0x07, 0x00),
        ("mbc3-unsupported-ram", 0x12, 0x06, 0x05),
        ("mbc5-special-rom-size", 0x19, 0x52, 0x00),
        ("mbc5-unsupported-ram", 0x1A, 0x08, 0x05),
    ] {
        assert!(
            validate_direct_gb_rom(&build_direct_mapper_rom(cartridge_type, rom_size, ram_size,))
                .is_err(),
            "{label}"
        );
    }
}

#[test]
fn strict_state_and_fresh_baseline_are_required() -> Result<()> {
    let (_directory, source_path, _) = write_direct_rom("tas-direct-gb-state")?;
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    let mut legacy = project.start_state().to_vec();
    legacy[8..12].copy_from_slice(&12u32.to_le_bytes());
    assert!(loader.load_session(&legacy).is_err());

    let mut changed = project.start_state().to_vec();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    assert!(loader.load_session(&changed).is_err());
    Ok(())
}

#[test]
fn identity_rejects_runtime_facts_outside_the_profile() -> Result<()> {
    let (_directory, source_path, source_bytes) = write_direct_rom("tas-direct-gb-runtime")?;
    let backend = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &source_path,
        &source_path,
        None,
        BackendLoadConfig {
            gb_hardware_mode_preference: HardwareModePreference::Auto,
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )?
    .backend;
    let start_state = backend.encode_state_bytes()?;
    assert!(direct_gb_tas_identity(&backend, &source_bytes, &start_state).is_err());
    Ok(())
}

fn profile_backend(
    setup: GbTasLoadSetup,
    raw_source_media_len: usize,
    persistent_load: GbPersistentLoadOutcome,
    external_boot_rom: bool,
) -> EmuBackend {
    let source = build_gb_test_rom();
    let path = PathBuf::from("profile.gb");
    let emu = if external_boot_rom {
        zeff_gb_core::emulator::Emulator::from_rom_data_with_boot_rom(
            &source,
            setup.requested_hardware_mode,
            &[0; 0x100],
        )
        .unwrap()
    } else {
        zeff_gb_core::emulator::Emulator::from_rom_data(&source, setup.requested_hardware_mode)
            .unwrap()
    };
    let provenance = GbTasLoadProvenanceSeed::new(
        zeff_firmware::sha256_bytes(&source),
        raw_source_media_len,
        &path,
        &path,
        setup,
    )
    .finish(
        persistent_load,
        emu.hardware_mode(),
        emu.sample_rate(),
        emu.has_boot_rom(),
    );
    let mut backend = EmuBackend::Gb(Box::new(GbBackend::with_load_provenance(
        emu,
        path.clone(),
        path,
        provenance,
    )));
    backend.set_firmware_manifests(
        crate::emu_backend::firmware::default_firmware_manifests_for_active_system(
            ActiveSystem::GameBoy,
        ),
    );
    backend
}

#[test]
fn runtime_rejects_every_direct_gb_profile_deviation() {
    let setup = GbTasLoadSetup {
        loaded_from_source_path: true,
        requested_hardware_mode: HardwareModePreference::ForceDmg,
        ..GbTasLoadSetup::default()
    };
    assert!(
        validate_direct_gb_tas_runtime(
            &profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false,),
            false,
        )
        .is_ok()
    );

    let deviations = [
        (
            "route",
            GbTasLoadSetup {
                loaded_from_source_path: false,
                ..setup
            },
            32 * 1024,
            GbPersistentLoadOutcome::Absent,
            false,
        ),
        (
            "length",
            setup,
            32 * 1024 + 1,
            GbPersistentLoadOutcome::Absent,
            false,
        ),
        (
            "mods",
            GbTasLoadSetup {
                any_mod_enabled: true,
                ..setup
            },
            32 * 1024,
            GbPersistentLoadOutcome::Absent,
            false,
        ),
        (
            "persistence",
            setup,
            32 * 1024,
            GbPersistentLoadOutcome::Loaded,
            false,
        ),
        (
            "boot",
            setup,
            32 * 1024,
            GbPersistentLoadOutcome::Absent,
            true,
        ),
        (
            "hardware",
            GbTasLoadSetup {
                requested_hardware_mode: HardwareModePreference::Auto,
                ..setup
            },
            32 * 1024,
            GbPersistentLoadOutcome::Absent,
            false,
        ),
    ];
    for (label, setup, len, persistent_load, external_boot_rom) in deviations {
        assert!(
            validate_direct_gb_tas_runtime(
                &profile_backend(setup, len, persistent_load, external_boot_rom),
                false,
            )
            .is_err(),
            "{label}"
        );
    }

    let mut sample_rate = profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false);
    sample_rate.set_sample_rate(44_100);
    assert!(validate_direct_gb_tas_runtime(&sample_rate, false).is_err());

    let mut palette = profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false);
    let EmuBackend::Gb(gb) = &mut palette else {
        unreachable!();
    };
    gb.emu
        .set_dmg_palette_preset(zeff_gb_core::hardware::ppu::DmgPalettePreset::Mint);
    assert!(validate_direct_gb_tas_runtime(&palette, false).is_err());

    let mut serial = profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false);
    assert!(
        serial.set_game_boy_serial_device(zeff_gb_core::hardware::GameBoySerialDevice::Printer)
    );
    assert!(validate_direct_gb_tas_runtime(&serial, false).is_err());

    let mut firmware = profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false);
    firmware.set_firmware_manifests(Vec::new());
    assert!(validate_direct_gb_tas_runtime(&firmware, false).is_err());

    let cheats = profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false);
    assert!(validate_direct_gb_tas_runtime(&cheats, true).is_err());
}

#[test]
fn branch_scope_rejects_non_p1_input_events_and_nondefault_start_metadata() -> Result<()> {
    let (_directory, source_path, _) = write_direct_rom("tas-direct-gb-branch")?;
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    let mut p2 = project.clone();
    p2.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                players: [
                    TasControllerInput::default(),
                    TasControllerInput {
                        buttons: 1,
                        dpad: 0,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        )
    })?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&p2, "main").is_err());

    let mut high_bits = project.clone();
    high_bits.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 0x10,
                        dpad: 0,
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
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&high_bits, "main").is_err());

    let mut special_input = project.clone();
    special_input.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                zapper: crate::tas_project::TasZapperInput {
                    enabled: true,
                    ..crate::tas_project::TasZapperInput::default()
                },
                ..TasInputFrame::default()
            },
        )
    })?;
    assert!(
        DirectGbTasExecutionLoader::validate_project_branch_scope(&special_input, "main").is_err()
    );

    let mut event = project.clone();
    event.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 0 }])
    })?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&event, "main").is_err());

    let identity = project.identity().clone();
    let start_state = project.start_state().to_vec();
    let replay_start = ReplayStartMetadata {
        game_boy_link_tick: Some(0),
        ..ReplayStartMetadata::default()
    };
    let linked = TasProject::new(
        "linked",
        identity,
        start_state,
        replay_start,
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&linked, "main").is_err());
    Ok(())
}

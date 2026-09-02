use super::*;
use crate::emu_backend::nes::{NesPersistentLoadOutcome, NesTasInitialInput};
use crate::emu_backend::{BackendLoadConfig, load_backend_from_rom_source};
use std::io::Write;

fn valid_facts() -> DirectNesProfileFacts {
    DirectNesProfileFacts {
        system: ActiveSystem::Nes,
        identity_metadata_matches: true,
        provenance: Some(NesTasLoadProvenance {
            raw_source_media_sha256: [7; 32],
            direct_nes_file: true,
            sync_config_sha256: crate::emu_backend::loader::direct_nes_tas_sync_config_sha256().0,
            any_mod_enabled: false,
            any_mod_applied: false,
            persistent_load: NesPersistentLoadOutcome::Absent,
            initial_input: NesTasInitialInput {
                buttons: 0,
                dpad: 0,
            },
            configured_sample_rate: None,
            initial_sample_rate: 48_000,
        }),
        current_sample_rate: Some(48_000),
        effective_media_sha256: [7; 32],
        battery_backed: false,
        battery_state_available: false,
        firmware_present: false,
        standard_console_hardware: true,
        supported_controller_topology: true,
        removable_media_present: false,
        cheats_present: false,
    }
}

fn rejection(facts: &DirectNesProfileFacts) -> Rejected {
    validate_direct_nes_profile(facts).unwrap_err()
}

#[test]
fn unsupported_system_is_typed() {
    let mut facts = valid_facts();
    facts.system = ActiveSystem::GameBoy;
    assert_eq!(rejection(&facts), Rejected::UnsupportedSystem);
}

#[test]
fn missing_load_provenance_is_typed() {
    let mut facts = valid_facts();
    facts.provenance = None;
    assert_eq!(rejection(&facts), Rejected::LoadProvenanceUnavailable);
}

#[test]
fn identity_metadata_mismatch_is_typed() {
    let mut facts = valid_facts();
    facts.identity_metadata_matches = false;
    assert_eq!(rejection(&facts), Rejected::IdentityMetadataMismatch);
}

#[test]
fn non_direct_source_is_typed() {
    let mut facts = valid_facts();
    facts.provenance.as_mut().unwrap().direct_nes_file = false;
    assert_eq!(rejection(&facts), Rejected::DirectNesFileRequired);
}

#[test]
fn source_effective_media_mismatch_is_typed() {
    let mut facts = valid_facts();
    facts.effective_media_sha256[0] ^= 1;
    assert_eq!(rejection(&facts), Rejected::SourceMediaMismatch);
}

#[test]
fn enabled_or_applied_mods_are_typed() {
    for (enabled, applied) in [(true, false), (false, true)] {
        let mut facts = valid_facts();
        let provenance = facts.provenance.as_mut().unwrap();
        provenance.any_mod_enabled = enabled;
        provenance.any_mod_applied = applied;
        assert_eq!(rejection(&facts), Rejected::ModsEnabledOrApplied);
    }
}

#[test]
fn unsupported_persistent_state_is_typed() {
    let mut loaded_without_battery = valid_facts();
    loaded_without_battery
        .provenance
        .as_mut()
        .unwrap()
        .persistent_load = NesPersistentLoadOutcome::Loaded;
    assert_eq!(
        rejection(&loaded_without_battery),
        Rejected::PersistentStateNotAbsent
    );

    let mut unknown = valid_facts();
    unknown.provenance.as_mut().unwrap().persistent_load = NesPersistentLoadOutcome::Unknown;
    assert_eq!(rejection(&unknown), Rejected::PersistentStateNotAbsent);

    let mut missing_battery = valid_facts();
    missing_battery.battery_backed = true;
    missing_battery
        .provenance
        .as_mut()
        .unwrap()
        .sync_config_sha256 =
        crate::emu_backend::loader::direct_nes_battery_tas_sync_config_sha256().0;
    assert_eq!(
        rejection(&missing_battery),
        Rejected::PersistentStateNotAbsent
    );
}

#[test]
fn exact_battery_profile_remains_fenced_from_in_place_linking() {
    for outcome in [
        NesPersistentLoadOutcome::Absent,
        NesPersistentLoadOutcome::Loaded,
    ] {
        let mut facts = valid_facts();
        facts.battery_backed = true;
        facts.battery_state_available = true;
        let provenance = facts.provenance.as_mut().unwrap();
        provenance.persistent_load = outcome;
        provenance.sync_config_sha256 =
            crate::emu_backend::loader::direct_nes_battery_tas_sync_config_sha256().0;
        assert_eq!(rejection(&facts), Rejected::PersistentStateNotAbsent);
    }
}

#[test]
fn non_neutral_initial_input_is_typed() {
    for input in [(1, 0), (0, 1)] {
        let mut facts = valid_facts();
        facts.provenance.as_mut().unwrap().initial_input = NesTasInitialInput {
            buttons: input.0,
            dpad: input.1,
        };
        assert_eq!(rejection(&facts), Rejected::NonNeutralInitialInput);
    }
}

#[test]
fn every_non_default_sample_rate_fact_is_typed() {
    let mut configured = valid_facts();
    configured
        .provenance
        .as_mut()
        .unwrap()
        .configured_sample_rate = Some(44_100);
    assert_eq!(rejection(&configured), Rejected::NonDefaultSampleRate);

    let mut initial = valid_facts();
    initial.provenance.as_mut().unwrap().initial_sample_rate = 44_100;
    assert_eq!(rejection(&initial), Rejected::NonDefaultSampleRate);

    let mut current = valid_facts();
    current.current_sample_rate = Some(44_100);
    assert_eq!(rejection(&current), Rejected::NonDefaultSampleRate);
}

#[test]
fn firmware_is_typed() {
    let mut facts = valid_facts();
    facts.firmware_present = true;
    assert_eq!(rejection(&facts), Rejected::FirmwarePresent);
}

#[test]
fn non_standard_controller_topology_is_typed() {
    let mut facts = valid_facts();
    facts.supported_controller_topology = false;
    assert_eq!(rejection(&facts), Rejected::NonStandardControllerTopology);
}

#[test]
fn non_standard_console_hardware_is_typed() {
    let mut facts = valid_facts();
    facts.standard_console_hardware = false;
    assert_eq!(rejection(&facts), Rejected::NonStandardConsoleHardware);
}

#[test]
fn removable_media_is_typed() {
    let mut facts = valid_facts();
    facts.removable_media_present = true;
    assert_eq!(rejection(&facts), Rejected::RemovableMediaPresent);
}

#[test]
fn cheats_are_typed() {
    let mut facts = valid_facts();
    facts.cheats_present = true;
    assert_eq!(rejection(&facts), Rejected::CheatsPresent);
}

#[test]
fn state_capture_failure_or_frame_mutation_is_typed() {
    let capture = capture_current_state(|| 9, || anyhow::bail!("encode failed"));
    assert_eq!(capture.unwrap_err(), Rejected::StateWitnessUnavailable);

    let mut frames = [9, 10].into_iter();
    let capture = capture_current_state(|| frames.next().unwrap(), || Ok(vec![1, 2, 3]));
    assert_eq!(capture.unwrap_err(), Rejected::StateWitnessUnavailable);
}

#[test]
fn direct_file_witness_reports_zapper_topology() {
    let directory = crate::test_support::test_directory("tas-witness-zapper").unwrap();
    let rom_path = directory.path().join("game.nes");
    std::fs::write(&rom_path, crate::test_support::build_nes_test_rom()).unwrap();
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig {
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;

    assert!(build_tas_witness(&backend, false, TasExecutionProfile::DirectNesCartridge).is_ok());
    backend.set_zapper_state(true, false, false, None);
    assert!(build_tas_witness(&backend, false, TasExecutionProfile::DirectNesCartridge).is_ok());
}

#[test]
fn selected_zip_member_witness_binds_archive_and_member() {
    let directory = crate::test_support::test_directory("tas-witness-nes-zip").unwrap();
    let archive_path = directory.path().join("games.zip");
    let rom = crate::test_support::build_nes_test_rom();
    let file = std::fs::File::create(&archive_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file("folder/game.nes", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(&rom).unwrap();
    writer.finish().unwrap();
    let archive_bytes = std::fs::read(&archive_path).unwrap();
    let rom_path = archive_path.join("folder/game.nes");
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &archive_path,
        &rom_path,
        Some(rom.clone()),
        BackendLoadConfig {
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    assert!(
        !backend
            .nes_tas_load_provenance()
            .unwrap()
            .load
            .direct_nes_file
    );
    let witness =
        build_tas_witness(&backend, false, TasExecutionProfile::DirectNesCartridge).unwrap();

    assert_eq!(
        witness.source_media_sha256,
        TasDigest::from_bytes(&archive_bytes)
    );
    assert_eq!(witness.effective_media_sha256, TasDigest::from_bytes(&rom));
    assert_eq!(
        witness.sync_config_sha256,
        crate::emu_backend::loader::zip_nes_tas_sync_config_sha256("folder/game.nes")
    );
    backend.step_frame();
    assert!(build_tas_witness(&backend, false, TasExecutionProfile::DirectNesCartridge).is_ok());
}

#[test]
fn direct_battery_witness_reports_project_owned_sram_but_rejects_in_place_linking() {
    let directory = crate::test_support::test_directory("tas-witness-nes-battery").unwrap();
    let rom_path = directory.path().join("game.nes");
    let rom = crate::test_support::build_nes_battery_test_rom();
    let sram = crate::test_support::nes_battery_test_bytes(&rom, 0xA5);
    std::fs::write(&rom_path, &rom).unwrap();
    std::fs::write(rom_path.with_extension("sav"), &sram).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig::default(),
    )
    .unwrap()
    .backend;

    let provenance = backend.nes_tas_load_provenance().unwrap();
    assert_eq!(
        provenance.load.sync_config_sha256,
        crate::emu_backend::loader::direct_nes_battery_tas_sync_config_sha256().0
    );
    assert_eq!(
        backend.nes().unwrap().emu.dump_battery_sram().unwrap(),
        sram
    );
    assert_eq!(
        observe_loaded_profile(&backend, false, TasExecutionProfile::DirectNesCartridge)
            .persistent_state_absent,
        Some(false)
    );
    assert_eq!(
        build_tas_witness(&backend, false, TasExecutionProfile::DirectNesCartridge).unwrap_err(),
        Rejected::PersistentStateNotAbsent
    );
}

#[test]
fn direct_gb_battery_witness_remains_fenced_from_in_place_linking() {
    let directory = crate::test_support::test_directory("tas-witness-gb-battery").unwrap();
    let rom_path = directory.path().join("game.gb");
    let mut rom = crate::test_support::build_gb_test_rom();
    rom.resize(256 * 1024, 0);
    rom[0x147] = 0x03;
    rom[0x148] = 0x03;
    rom[0x149] = 0x03;
    let sram = vec![0xA5; 32 * 1024];
    std::fs::write(&rom_path, &rom).unwrap();
    std::fs::write(rom_path.with_extension("sav"), &sram).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig {
            gb_hardware_mode_preference:
                zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceDmg,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;

    assert_eq!(backend.gb_tas_battery_bytes().unwrap(), sram);
    assert_eq!(
        observe_loaded_profile(&backend, false, TasExecutionProfile::DirectGbCartridgeDmg)
            .persistent_state_absent,
        Some(false)
    );
    assert!(build_tas_witness(&backend, false, TasExecutionProfile::DirectGbCartridgeDmg).is_err());
}

#[test]
fn direct_cgb_battery_witness_remains_fenced_from_in_place_linking() {
    let directory = crate::test_support::test_directory("tas-witness-cgb-battery").unwrap();
    let rom_path = directory.path().join("game.gbc");
    let mut rom = crate::test_support::build_gb_test_rom();
    rom.resize(256 * 1024, 0);
    rom[0x143] = 0xC0;
    rom[0x147] = 0x03;
    rom[0x148] = 0x03;
    rom[0x149] = 0x03;
    let sram = vec![0x6C; 32 * 1024];
    std::fs::write(&rom_path, &rom).unwrap();
    std::fs::write(rom_path.with_extension("sav"), &sram).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig {
            gb_hardware_mode_preference:
                zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceCgb,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;

    assert_eq!(backend.gb_tas_battery_bytes().unwrap(), sram);
    assert_eq!(
        observe_loaded_profile(&backend, false, TasExecutionProfile::DirectGbCartridgeCgb)
            .persistent_state_absent,
        Some(false)
    );
    assert!(build_tas_witness(&backend, false, TasExecutionProfile::DirectGbCartridgeCgb).is_err());
}

#[test]
fn direct_ws_battery_witness_is_exact_and_rejects_in_place_linking() {
    use zeff_ws_core::hardware::cartridge::{SaveKind, compute_footer_checksum};

    let directory = crate::test_support::test_directory("tas-witness-ws-battery").unwrap();
    let rom_path = directory.path().join("game.ws");
    let mut rom = vec![0x90; 128 * 1024];
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer..].fill(0);
    rom[footer + 4] = 0x01;
    rom[footer + 5] = 0x20;
    rom[footer + 6] = 1;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    let save = vec![0x6A; SaveKind::Eeprom1K.size()];
    std::fs::write(&rom_path, &rom).unwrap();
    std::fs::write(rom_path.with_extension("sav"), &save).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::WonderSwan,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig::default(),
    )
    .unwrap()
    .backend;

    assert_eq!(backend.ws_tas_battery_bytes().unwrap(), save);
    assert_eq!(
        observe_loaded_profile(&backend, false, TasExecutionProfile::DirectWsCartridge)
            .persistent_state_absent,
        Some(false)
    );
    assert_eq!(
        build_tas_witness(&backend, false, TasExecutionProfile::DirectWsCartridge).unwrap_err(),
        Rejected::PersistentStateNotAbsent
    );
    let persistence = TasPersistenceContract::WsBattery {
        save_kind: SaveKind::Eeprom1K,
        byte_len: save.len() as u64,
        initial_sha256: TasDigest::from_bytes(&save),
        target_baseline: crate::emu_thread::TasPersistenceBaseline::Missing,
    };
    assert!(
        build_tas_witness_for_persistence(
            &backend,
            false,
            TasExecutionProfile::DirectWsCartridge,
            persistence,
        )
        .is_ok()
    );
    let wrong_kind = TasPersistenceContract::WsBattery {
        save_kind: SaveKind::Sram32K,
        byte_len: save.len() as u64,
        initial_sha256: TasDigest::from_bytes(&save),
        target_baseline: crate::emu_thread::TasPersistenceBaseline::Missing,
    };
    assert_eq!(
        build_tas_witness_for_persistence(
            &backend,
            false,
            TasExecutionProfile::DirectWsCartridge,
            wrong_kind,
        )
        .unwrap_err(),
        Rejected::PersistentStateNotAbsent
    );
}

#[test]
fn zip_battery_witness_binds_identity_but_rejects_in_place_linking() {
    let directory = crate::test_support::test_directory("tas-witness-nes-zip-battery").unwrap();
    let archive_path = directory.path().join("games.zip");
    let rom = crate::test_support::build_nes_battery_test_rom();
    let file = std::fs::File::create(&archive_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file("folder/game.nes", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(&rom).unwrap();
    writer.finish().unwrap();
    let rom_path = archive_path.join("folder/game.nes");
    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &archive_path,
        &rom_path,
        Some(rom),
        BackendLoadConfig::default(),
    )
    .unwrap()
    .backend;

    let provenance = backend.nes_tas_load_provenance().unwrap();
    assert_eq!(
        provenance.load.sync_config_sha256,
        crate::emu_backend::loader::zip_nes_battery_tas_sync_config_sha256("folder/game.nes").0
    );
    assert_eq!(
        build_tas_witness(&backend, false, TasExecutionProfile::DirectNesCartridge).unwrap_err(),
        Rejected::PersistentStateNotAbsent
    );
}

#[test]
fn readiness_observation_preserves_load_and_current_sample_rates() {
    let directory = crate::test_support::test_directory("tas-readiness-observation").unwrap();
    let rom_path = directory.path().join("game.nes");
    std::fs::write(&rom_path, crate::test_support::build_nes_test_rom()).unwrap();
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig {
            sample_rate: Some(44_100),
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    backend.set_sample_rate(48_000);

    let observation =
        observe_loaded_profile(&backend, false, TasExecutionProfile::DirectNesCartridge);

    assert_eq!(observation.configured_at_load_sample_rate, Some(44_100));
    assert_eq!(observation.initial_sample_rate, Some(44_100));
    assert_eq!(observation.current_sample_rate, Some(48_000));
}

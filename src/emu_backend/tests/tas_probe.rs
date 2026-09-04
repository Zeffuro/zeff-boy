use std::path::PathBuf;

use super::*;
use crate::emu_backend::EmuBackend;
use crate::emu_backend::capabilities::{TasInputModel, TasPersistentStateIdentity};

#[test]
fn tas_core_probe_keeps_standard_sega_mapper_persistence_unknown() {
    let probe = build_sms_backend().capabilities().tas_execution_primitives;

    assert!(probe.system_identity_observed);
    assert_eq!(probe.source_media_identity, None);
    assert!(!probe.source_media_identity_observed);
    assert!(probe.effective_media_identity_observed);
    assert!(!probe.firmware_identity_observed);
    assert!(probe.supports_state_restore);
    assert_eq!(
        probe.persistent_state,
        TasPersistentStateIdentity::Unknown {
            size: zeff_sega8_core::hardware::constants::SMS_CARTRIDGE_RAM_SIZE,
        }
    );
    assert_eq!(
        probe.input_model,
        TasInputModel::StandardDigitalPads { max_players: 2 }
    );
}

#[test]
fn loader_owned_sms_source_identity_survives_state_restore_and_reaches_worker_status() {
    let rom = build_sms_test_rom();
    let expected = crate::emu_backend::capabilities::TasSourceMediaIdentity::new(
        zeff_firmware::sha256_bytes(&rom),
        rom.len(),
    );
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::MasterSystem,
        &PathBuf::from("synthetic-container.bin"),
        &PathBuf::from("synthetic-game.sms"),
        Some(rom),
        BackendLoadConfig {
            sample_rate: Some(44_100),
            ..BackendLoadConfig::default()
        },
    )
    .expect("synthetic SMS loader should initialize")
    .backend;

    let before = backend.capabilities().tas_execution_primitives;
    assert_eq!(before.source_media_identity, Some(expected));
    assert!(before.source_media_identity_observed);
    assert!(!before.direct_runtime_profile_requirements_match);
    let provenance = backend
        .sega8()
        .and_then(|sega8| sega8.sms_tas_load_provenance())
        .expect("SMS loader should retain TAS provenance");
    assert!(!provenance.load.direct_sms_file);
    assert_eq!(provenance.load.raw_source_media_sha256, expected.sha256);
    assert_eq!(provenance.load.raw_source_media_len, expected.byte_len);
    assert_eq!(provenance.load.configured_sample_rate, Some(44_100));
    assert_eq!(provenance.load.initial_sample_rate, 44_100);
    assert_eq!(provenance.current_sample_rate, 44_100);

    let state = backend
        .encode_state_bytes()
        .expect("SMS backend should encode synthetic state");
    backend.step_frame();
    backend
        .load_state_from_bytes(state)
        .expect("SMS backend should restore synthetic state");
    assert_eq!(
        backend
            .capabilities()
            .tas_execution_primitives
            .source_media_identity,
        Some(expected)
    );

    let mut worker = crate::emu_thread::EmuThread::spawn(backend, false);
    assert_eq!(
        worker
            .capabilities()
            .tas_execution_primitives
            .source_media_identity,
        Some(expected)
    );
    worker.shutdown();
}

#[test]
fn direct_sms_loader_retains_the_unmodified_direct_route() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("sms-tas-provenance")?;
    let path = directory.path().join("game.sms");
    let rom = build_sms_test_rom();
    std::fs::write(&path, &rom)?;
    let backend = load_backend_from_rom_source(
        ActiveSystem::MasterSystem,
        &path,
        &path,
        None,
        BackendLoadConfig::default(),
    )?
    .backend;

    let provenance = backend
        .sega8()
        .and_then(|sega8| sega8.sms_tas_load_provenance())
        .expect("direct SMS loader should retain TAS provenance");
    assert!(provenance.load.direct_sms_file);
    assert_eq!(
        provenance.load.raw_source_media_sha256,
        zeff_firmware::sha256_bytes(&rom)
    );
    assert_eq!(provenance.load.raw_source_media_len, rom.len());
    assert!(!provenance.load.any_mod_enabled);
    assert!(!provenance.load.any_mod_applied);
    assert_eq!(provenance.load.initial_input, None);
    assert_eq!(provenance.load.configured_sample_rate, None);
    assert_eq!(
        provenance.load.initial_sample_rate,
        zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE
    );
    assert_eq!(
        backend
            .capabilities()
            .tas_execution_primitives
            .source_media_identity,
        Some(
            crate::emu_backend::capabilities::TasSourceMediaIdentity::new(
                zeff_firmware::sha256_bytes(&rom),
                rom.len()
            )
        )
    );
    assert!(
        !backend
            .capabilities()
            .tas_execution_primitives
            .direct_runtime_profile_requirements_match
    );
    Ok(())
}

#[test]
fn tas_core_probe_classifies_codemasters_header_ram_as_volatile() {
    let probe = build_sega8_backend(build_codemasters_rom(), None)
        .capabilities()
        .tas_execution_primitives;

    assert_eq!(
        probe.persistent_state,
        TasPersistentStateIdentity::VolatileOnly { size: 0x2000 }
    );
}

#[test]
fn tas_core_probe_classifies_rom_only_mapper_as_absent() {
    let probe = build_sega8_backend(
        build_sms_test_rom(),
        Some(zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Korean),
    )
    .capabilities()
    .tas_execution_primitives;

    assert_eq!(probe.persistent_state, TasPersistentStateIdentity::Absent);
}

#[test]
fn tas_core_probe_reports_coleco_identity_blockers_and_semantic_controller_model() {
    let probe = build_coleco_backend()
        .capabilities()
        .tas_execution_primitives;

    assert!(probe.system_identity_observed);
    assert_eq!(probe.source_media_identity, None);
    assert!(!probe.source_media_identity_observed);
    assert!(probe.effective_media_identity_observed);
    assert!(!probe.firmware_identity_observed);
    assert!(probe.supports_state_restore);
    assert_eq!(probe.persistent_state, TasPersistentStateIdentity::Absent);
    assert_eq!(
        probe.input_model,
        TasInputModel::ColecoStandardController { max_players: 2 }
    );
}

#[test]
fn tas_core_probe_observes_a_loaded_coleco_firmware_identity() {
    let mut backend = build_coleco_backend();
    backend.set_firmware_manifests(vec![
        zeff_emu_common::replay::ReplayFirmwareManifest::External {
            firmware_id: "coleco.vision.bios".to_owned(),
            variant: Some("synthetic-retail-catalog-match".to_owned()),
            sha256: [0x5A; 32],
        },
    ]);

    let probe = backend.capabilities().tas_execution_primitives;
    assert!(probe.firmware_identity_observed);
    assert_eq!(probe.source_media_identity, None);
    assert!(!probe.source_media_identity_observed);
    assert_eq!(probe.persistent_state, TasPersistentStateIdentity::Absent);
    assert_eq!(
        probe.input_model,
        TasInputModel::ColecoStandardController { max_players: 2 }
    );
}

#[test]
fn tas_core_probe_reports_existing_direct_profile_primitives() {
    let nes = build_nes_backend().capabilities().tas_execution_primitives;
    let gb = build_gb_backend().capabilities().tas_execution_primitives;

    for probe in [nes, gb] {
        assert!(probe.system_identity_observed);
        assert!(!probe.source_media_identity_observed);
        assert!(probe.effective_media_identity_observed);
        assert!(probe.supports_state_restore);
        assert_eq!(probe.persistent_state, TasPersistentStateIdentity::Absent);
    }
    assert_eq!(
        nes.input_model,
        TasInputModel::StandardDigitalPads { max_players: 2 }
    );
    assert_eq!(gb.input_model, TasInputModel::GameBoyJoypad);
}

#[test]
fn loader_owned_coleco_source_identity_survives_state_restore_and_reaches_worker_status() {
    let mut rom = vec![0; 8 * 1024];
    rom[..4].copy_from_slice(&[0xAA, 0x55, 0x12, 0x34]);
    let expected = crate::emu_backend::capabilities::TasSourceMediaIdentity::new(
        zeff_firmware::sha256_bytes(&rom),
        rom.len(),
    );
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Coleco,
        &PathBuf::from("synthetic-container.bin"),
        &PathBuf::from("synthetic-game.col"),
        Some(rom),
        BackendLoadConfig {
            sample_rate: Some(44_100),
            coleco_bios_override: Some(&TEST_COLECO_BIOS),
            ..BackendLoadConfig::default()
        },
    )
    .expect("synthetic Coleco loader should initialize")
    .backend;

    let before = backend.capabilities().tas_execution_primitives;
    assert_eq!(before.source_media_identity, Some(expected));
    assert!(before.source_media_identity_observed);
    assert!(before.effective_media_identity_observed);
    assert!(before.firmware_identity_observed);
    let provenance = backend
        .coleco()
        .and_then(|coleco| coleco.tas_load_provenance())
        .expect("Coleco loader should retain TAS provenance");
    assert!(!provenance.load.direct_col_file);
    assert_eq!(provenance.load.raw_source_media_sha256, expected.sha256);
    assert_eq!(provenance.load.raw_source_media_len, expected.byte_len);
    assert_eq!(provenance.load.configured_sample_rate, Some(44_100));
    assert_eq!(provenance.load.initial_sample_rate, 44_100);
    assert_eq!(provenance.current_sample_rate, 44_100);
    assert!(!before.direct_runtime_profile_requirements_match);

    let state = backend
        .encode_state_bytes()
        .expect("Coleco backend should encode synthetic state");
    backend.step_frame();
    backend
        .load_state_from_bytes(state)
        .expect("Coleco backend should restore synthetic state");
    assert_eq!(
        backend
            .capabilities()
            .tas_execution_primitives
            .source_media_identity,
        Some(expected)
    );

    let mut worker = crate::emu_thread::EmuThread::spawn(backend, false);
    assert_eq!(
        worker
            .capabilities()
            .tas_execution_primitives
            .source_media_identity,
        Some(expected)
    );
    worker.shutdown();
}

#[test]
fn direct_coleco_loader_owns_route_configuration_and_neutral_controller_facts() -> anyhow::Result<()>
{
    let directory = crate::test_support::test_directory("coleco-tas-provenance")?;
    let path = directory.path().join("game.col");
    let mut rom = vec![0; 8 * 1024];
    rom[..2].copy_from_slice(&[0xAA, 0x55]);
    std::fs::write(&path, &rom)?;

    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Coleco,
        &path,
        &path,
        None,
        BackendLoadConfig {
            coleco_bios_override: Some(&TEST_COLECO_BIOS),
            ..BackendLoadConfig::default()
        },
    )?
    .backend;
    let provenance = backend
        .coleco()
        .and_then(|coleco| coleco.tas_load_provenance())
        .expect("direct Coleco load should retain TAS provenance");
    assert!(provenance.load.direct_col_file);
    assert_eq!(
        provenance.load.raw_source_media_sha256,
        zeff_firmware::sha256_bytes(&rom)
    );
    assert_eq!(provenance.load.raw_source_media_len, rom.len());
    assert!(!provenance.load.any_mod_enabled);
    assert!(!provenance.load.any_mod_applied);
    assert_eq!(provenance.load.initial_input, None);
    assert_eq!(provenance.load.configured_sample_rate, None);
    assert_eq!(
        provenance.load.initial_sample_rate,
        zeff_coleco_core::constants::DEFAULT_SAMPLE_RATE
    );
    assert_eq!(
        provenance.current_controllers,
        [zeff_coleco_core::StandardController::default(); 2]
    );
    assert!(
        backend
            .capabilities()
            .tas_execution_primitives
            .direct_runtime_profile_requirements_match
    );

    backend.set_sample_rate(44_100);
    assert_eq!(
        backend
            .coleco()
            .and_then(|coleco| coleco.tas_load_provenance())
            .expect("Coleco provenance should survive host reconfiguration")
            .current_sample_rate,
        44_100
    );
    assert!(
        !backend
            .capabilities()
            .tas_execution_primitives
            .direct_runtime_profile_requirements_match
    );
    Ok(())
}

fn build_sega8_backend(
    rom: Vec<u8>,
    mapper_kind: Option<zeff_sega8_core::hardware::cartridge::Sega8MapperKind>,
) -> EmuBackend {
    let config = zeff_sega8_core::emulator::Sega8LoadConfig::new(44_100)
        .with_system_hint(zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem)
        .with_mapper_kind(mapper_kind);
    let emu = zeff_sega8_core::emulator::Emulator::new_with_config(&rom, config)
        .expect("synthetic Sega 8-bit ROM should initialize");
    EmuBackend::from_sega8(emu, PathBuf::from("tas-probe.sms"))
}

fn build_codemasters_rom() -> Vec<u8> {
    let mut rom = vec![0xFF; 0x8000];
    let header = 0x7FE0;
    rom[header] = 2;
    rom[header + 1] = 0x31;
    rom[header + 2] = 0x08;
    rom[header + 3] = 0x93;
    rom[header + 4] = 0x10;
    rom[header + 5] = 0x59;
    rom[header + 6..header + 8].copy_from_slice(&0x1234u16.to_le_bytes());
    rom[header + 8..header + 10].copy_from_slice(&0xEDCCu16.to_le_bytes());
    rom[header + 10..header + 16].fill(0);
    rom
}

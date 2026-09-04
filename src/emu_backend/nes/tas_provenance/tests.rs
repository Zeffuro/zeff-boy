use std::path::{Path, PathBuf};

use super::*;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::test_support::{build_nes_test_rom, test_directory};

fn load_preloaded(rom: Vec<u8>, path: &Path, config: BackendLoadConfig) -> EmuBackend {
    load_backend_from_rom_source(ActiveSystem::Nes, path, path, Some(rom), config)
        .unwrap()
        .backend
}

#[test]
fn direct_clean_load_captures_neutral_provenance() {
    let root = test_directory("nes-tas-provenance-direct").unwrap();
    let rom_path = root.path().join("clean.NES");
    let rom = build_nes_test_rom();
    let expected_sha256 = zeff_firmware::sha256_bytes(&rom);
    std::fs::write(&rom_path, rom).unwrap();
    let backend = load_backend_from_rom_source(
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
    let view = backend.nes_tas_load_provenance().unwrap();

    assert_eq!(view.load.raw_source_media_sha256, expected_sha256);
    assert!(view.load.direct_nes_file);
    assert!(!view.load.any_mod_enabled);
    assert!(!view.load.any_mod_applied);
    assert_eq!(view.load.persistent_load, NesPersistentLoadOutcome::Absent);
    assert_eq!(
        view.load.initial_input,
        NesTasInitialInput {
            buttons: 0,
            dpad: 0,
        }
    );
    assert_eq!(view.load.configured_sample_rate, None);
    assert_eq!(view.load.initial_sample_rate, 48_000);
    assert_eq!(view.current_sample_rate, 48_000);
}

#[test]
fn same_path_preloaded_bytes_are_not_direct_file_provenance() {
    let backend = load_preloaded(
        build_nes_test_rom(),
        Path::new("preloaded.nes"),
        BackendLoadConfig {
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    );

    assert!(
        !backend
            .nes_tas_load_provenance()
            .unwrap()
            .load
            .direct_nes_file
    );
}

#[test]
fn modded_effective_media_keeps_raw_identity_and_mod_facts() {
    let raw = build_nes_test_rom();
    let raw_sha256 = zeff_firmware::sha256_bytes(&raw);
    let mut effective = raw.clone();
    *effective.last_mut().unwrap() ^= 0xFF;
    let emu = zeff_nes_core::emulator::Emulator::new(&effective, 48_000.0).unwrap();
    let provenance = NesTasLoadProvenanceSeed::new(
        raw_sha256,
        Path::new("modded.nes"),
        Path::new("modded.nes"),
        NesTasLoadSetup {
            any_mod_enabled: true,
            any_mod_applied: true,
            ..NesTasLoadSetup::default()
        },
    )
    .finish(NesPersistentLoadOutcome::Absent, false);
    let backend = EmuBackend::Nes(Box::new(super::super::NesBackend::with_load_provenance(
        emu,
        PathBuf::from("modded.nes"),
        PathBuf::from("modded.nes"),
        provenance,
    )));
    let view = backend.nes_tas_load_provenance().unwrap();

    assert_eq!(view.load.raw_source_media_sha256, raw_sha256);
    assert_ne!(view.load.raw_source_media_sha256, backend.rom_hash());
    assert!(view.load.any_mod_enabled);
    assert!(view.load.any_mod_applied);
}

#[test]
fn battery_save_load_is_recorded_as_loaded() {
    let root = test_directory("nes-tas-provenance-loaded").unwrap();
    let rom_path = root.path().join("battery.nes");
    let rom = crate::test_support::build_nes_battery_test_rom();
    let seed = zeff_nes_core::emulator::Emulator::new(&rom, 48_000.0).unwrap();
    let persistent_len = seed.dump_persistent_data().unwrap().len();
    std::fs::write(root.path().join("battery.sav"), vec![0xA5; persistent_len]).unwrap();

    let backend = load_preloaded(rom, &rom_path, BackendLoadConfig::default());

    assert_eq!(
        backend
            .nes_tas_load_provenance()
            .unwrap()
            .load
            .persistent_load,
        NesPersistentLoadOutcome::Loaded
    );
}

#[test]
fn battery_save_read_failure_is_recorded_as_unknown() {
    let root = test_directory("nes-tas-provenance-unknown").unwrap();
    let rom_path = root.path().join("battery.nes");
    std::fs::create_dir(root.path().join("battery.sav")).unwrap();

    let backend = load_preloaded(
        crate::test_support::build_nes_battery_test_rom(),
        &rom_path,
        BackendLoadConfig::default(),
    );

    assert_eq!(
        backend
            .nes_tas_load_provenance()
            .unwrap()
            .load
            .persistent_load,
        NesPersistentLoadOutcome::Unknown
    );
}

#[test]
fn non_neutral_initial_input_is_captured_exactly() {
    let backend = load_preloaded(
        build_nes_test_rom(),
        Path::new("input.nes"),
        BackendLoadConfig {
            initial_input: Some((0x35, 0x0A)),
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    );

    assert_eq!(
        backend
            .nes_tas_load_provenance()
            .unwrap()
            .load
            .initial_input,
        NesTasInitialInput {
            buttons: 0x35,
            dpad: 0x0A,
        }
    );
}

#[test]
fn current_sample_rate_tracks_runtime_mutation_without_changing_load_facts() {
    let mut backend = load_preloaded(
        build_nes_test_rom(),
        Path::new("audio.nes"),
        BackendLoadConfig {
            sample_rate: Some(44_100),
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    );
    let initial = *backend.nes_tas_load_provenance().unwrap().load;

    backend.set_sample_rate(96_000);
    let current = backend.nes_tas_load_provenance().unwrap();

    assert_eq!(current.load.configured_sample_rate, Some(44_100));
    assert_eq!(current.load.initial_sample_rate, 44_100);
    assert_eq!(current.current_sample_rate, 96_000);
    assert_eq!(*current.load, initial);
}

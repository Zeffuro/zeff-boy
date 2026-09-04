use std::path::{Path, PathBuf};

use super::*;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::test_support::{build_gb_test_rom, test_directory};

fn load_preloaded(rom: Vec<u8>, path: &Path, config: BackendLoadConfig) -> EmuBackend {
    load_backend_from_rom_source(ActiveSystem::GameBoy, path, path, Some(rom), config)
        .unwrap()
        .backend
}

fn battery_rom() -> Vec<u8> {
    let mut rom = build_gb_test_rom();
    rom[0x147] = 0x03;
    rom[0x149] = 0x02;
    rom
}

#[test]
fn direct_clean_load_captures_neutral_provenance() {
    let root = test_directory("gb-tas-provenance-direct").unwrap();
    let rom_path = root.path().join("clean.gb");
    let rom = build_gb_test_rom();
    let expected_sha256 = zeff_firmware::sha256_bytes(&rom);
    std::fs::write(&rom_path, &rom).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig {
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    let view = backend.gb_tas_load_provenance().unwrap();

    assert_eq!(view.load.raw_source_media_sha256, expected_sha256);
    assert_eq!(view.load.raw_source_media_len, rom.len());
    assert!(view.load.direct_gb_file);
    assert!(!view.load.direct_gbc_file);
    assert!(!view.load.any_mod_enabled);
    assert!(!view.load.any_mod_applied);
    assert_eq!(view.load.persistent_load, GbPersistentLoadOutcome::Absent);
    assert_eq!(
        view.load.initial_input,
        GbTasInitialInput {
            buttons: 0,
            dpad: 0
        }
    );
    assert_eq!(view.load.configured_sample_rate, None);
    assert_eq!(view.load.initial_sample_rate, view.current_sample_rate);
    assert_eq!(view.load.resolved_hardware_mode, view.current_hardware_mode);
    assert_eq!(
        view.cartridge_type,
        zeff_gb_core::hardware::types::CartridgeType::RomOnly
    );
    assert_eq!(view.rom_size, zeff_gb_core::hardware::types::RomSize::Kb32);
    assert_eq!(view.ram_size, zeff_gb_core::hardware::types::RamSize::None);
    assert!(!view.is_cgb_exclusive);
    assert!(!view.load.external_boot_rom_used);
    assert!(!view.has_external_boot_rom);
}

#[test]
fn preloaded_and_archive_routes_are_not_direct_file_provenance() {
    let preloaded = load_preloaded(
        build_gb_test_rom(),
        Path::new("preloaded.gb"),
        BackendLoadConfig {
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    );
    let root = test_directory("gb-tas-provenance-archive").unwrap();
    let archive_path = root.path().join("game.zip");
    let rom_path = root.path().join("game.gb");
    let archive = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &archive_path,
        &rom_path,
        Some(build_gb_test_rom()),
        BackendLoadConfig {
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;

    assert!(
        !preloaded
            .gb_tas_load_provenance()
            .unwrap()
            .load
            .direct_gb_file
    );
    assert!(
        !archive
            .gb_tas_load_provenance()
            .unwrap()
            .load
            .direct_gb_file
    );
}

#[test]
fn direct_gbc_source_is_distinct_from_direct_gb_source() {
    let root = test_directory("gb-tas-provenance-direct-gbc").unwrap();
    let rom_path = root.path().join("clean.gbc");
    let mut rom = build_gb_test_rom();
    rom[0x143] = 0xC0;
    std::fs::write(&rom_path, rom).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &rom_path,
        &rom_path,
        None,
        BackendLoadConfig {
            gb_hardware_mode_preference:
                zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceCgb,
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    let load = backend.gb_tas_load_provenance().unwrap().load;

    assert!(!load.direct_gb_file);
    assert!(load.direct_gbc_file);
}

#[test]
fn selected_zip_member_records_outer_source_and_member_profile() {
    let root = test_directory("gb-tas-provenance-zip-member").unwrap();
    let archive_path = root.path().join("games.zip");
    let rom_path = archive_path.join("folder/game.gb");
    let rom = build_gb_test_rom();
    let archive_sha256 = [0xA7; 32];
    let sync_config_sha256 = [0x5C; 32];
    let backend = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &archive_path,
        &rom_path,
        Some(rom.clone()),
        BackendLoadConfig {
            gb_hardware_mode_preference:
                zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceDmg,
            gb_tas_source_media: Some((archive_sha256, 12_345, sync_config_sha256)),
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    let load = backend.gb_tas_load_provenance().unwrap().load;

    assert!(load.direct_gb_file);
    assert!(!load.direct_gbc_file);
    assert_eq!(
        load.raw_source_media_sha256,
        zeff_firmware::sha256_bytes(&rom)
    );
    assert_eq!(load.tas_source_media_sha256, archive_sha256);
    assert_eq!(load.tas_source_media_len, 12_345);
    assert_eq!(load.tas_sync_config_sha256, sync_config_sha256);
    assert_eq!(
        backend.tas_source_media_identity().unwrap(),
        crate::emu_backend::capabilities::TasSourceMediaIdentity::new(archive_sha256, 12_345)
    );
}

#[test]
fn ordinary_battery_load_restores_sram() {
    let root = test_directory("gb-tas-provenance-loaded").unwrap();
    let rom_path = root.path().join("battery.gb");
    let rom = battery_rom();
    let seed = zeff_gb_core::emulator::Emulator::from_rom_data(
        &rom,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
    )
    .unwrap();
    let persistent_len = seed.dump_battery_sram().unwrap().len();
    std::fs::write(root.path().join("battery.sav"), vec![0xA5; persistent_len]).unwrap();

    let backend = load_preloaded(rom, &rom_path, BackendLoadConfig::default());

    assert_eq!(
        backend
            .gb_tas_load_provenance()
            .unwrap()
            .load
            .persistent_load,
        GbPersistentLoadOutcome::Loaded
    );
    assert_eq!(
        backend.gb().unwrap().emu.dump_battery_sram().unwrap(),
        vec![0xA5; persistent_len]
    );
}

#[test]
fn battery_opt_out_does_not_probe_or_restore_sram() {
    let root = test_directory("gb-tas-provenance-opt-out").unwrap();
    let rom_path = root.path().join("battery.gb");
    std::fs::create_dir(root.path().join("battery.sav")).unwrap();

    let backend = load_preloaded(
        battery_rom(),
        &rom_path,
        BackendLoadConfig {
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    );

    assert_eq!(
        backend
            .gb_tas_load_provenance()
            .unwrap()
            .load
            .persistent_load,
        GbPersistentLoadOutcome::Absent
    );
}

#[test]
fn current_runtime_facts_update_without_changing_load_facts() {
    let rom = build_gb_test_rom();
    let mut backend = load_preloaded(
        rom.clone(),
        Path::new("audio.gb"),
        BackendLoadConfig {
            sample_rate: Some(44_100),
            initial_input: Some((0x35, 0x0A)),
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    );
    let initial = *backend.gb_tas_load_provenance().unwrap().load;

    backend.set_sample_rate(96_000);
    let current = backend.gb_tas_load_provenance().unwrap();

    assert_eq!(current.load.configured_sample_rate, Some(44_100));
    assert_eq!(current.load.initial_sample_rate, 44_100);
    assert_eq!(
        current.load.initial_input,
        GbTasInitialInput {
            buttons: 0x05,
            dpad: 0x0A,
        }
    );
    assert_eq!(current.current_sample_rate, 96_000);
    assert_eq!(
        current.current_hardware_mode_preference,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto
    );
    assert_eq!(
        current.current_serial_device,
        zeff_gb_core::hardware::GameBoySerialDevice::Disconnected
    );
    assert_eq!(*current.load, initial);

    let mut state_source = zeff_gb_core::emulator::Emulator::from_rom_data(
        &rom,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceDmg,
    )
    .unwrap();
    state_source.set_game_boy_serial_device(zeff_gb_core::hardware::GameBoySerialDevice::Printer);
    state_source.set_dmg_palette_preset(zeff_gb_core::hardware::ppu::DmgPalettePreset::Mint);
    backend
        .load_state_from_bytes(state_source.encode_state_bytes().unwrap())
        .unwrap();
    let EmuBackend::Gb(gb) = &mut backend else {
        unreachable!();
    };
    gb.emu
        .set_dmg_palette_preset(zeff_gb_core::hardware::ppu::DmgPalettePreset::Mint);
    let current = backend.gb_tas_load_provenance().unwrap();

    assert_eq!(
        current.current_hardware_mode_preference,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceDmg
    );
    assert_eq!(
        current.current_serial_device,
        zeff_gb_core::hardware::GameBoySerialDevice::Printer
    );
    assert_eq!(
        current.dmg_palette_preset,
        zeff_gb_core::hardware::ppu::DmgPalettePreset::Mint
    );
    assert_eq!(current.current_sample_rate, 96_000);
    assert_eq!(*current.load, initial);
}

#[test]
fn external_boot_presence_is_witnessed_from_the_emulator() {
    let rom = build_gb_test_rom();
    let emu = zeff_gb_core::emulator::Emulator::from_rom_data_with_boot_rom(
        &rom,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceDmg,
        &[0; 0x100],
    )
    .unwrap();
    let path = PathBuf::from("external-boot.gb");
    let provenance = GbTasLoadProvenanceSeed::new(
        zeff_firmware::sha256_bytes(&rom),
        rom.len(),
        &path,
        &path,
        GbTasLoadSetup {
            loaded_from_source_path: true,
            ..GbTasLoadSetup::default()
        },
    )
    .finish(
        GbPersistentLoadOutcome::Absent,
        emu.hardware_mode(),
        emu.sample_rate(),
        emu.has_boot_rom(),
    );
    let backend = EmuBackend::Gb(Box::new(GbBackend::with_load_provenance(
        emu,
        path.clone(),
        path,
        provenance,
    )));
    let view = backend.gb_tas_load_provenance().unwrap();

    assert!(view.load.external_boot_rom_used);
    assert!(view.has_external_boot_rom);
}

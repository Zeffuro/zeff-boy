use super::*;

#[test]
fn shared_backend_loader_rejects_fds_without_firmware_dir() {
    let err = match load_backend_from_rom_source(
        ActiveSystem::Nes,
        &PathBuf::from("test.fds"),
        &PathBuf::from("test.fds"),
        Some(vec![
            0x55;
            zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE
        ]),
        BackendLoadConfig::default(),
    ) {
        Ok(_) => panic!("FDS app-level loading should remain guarded until firmware boot is wired"),
        Err(err) => err,
    };

    let message = err.to_string();
    assert!(message.contains("Famicom Disk System firmware is required"));
    assert!(message.contains("Settings > Firmware > Firmware directory"));
    assert!(message.contains("nintendo.fds.bios"));
}

#[test]
fn shared_backend_loader_uses_configured_fds_firmware_dir() {
    let firmware_dir = std::env::temp_dir().join(format!(
        "zeff_boy_empty_fds_firmware_dir_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&firmware_dir);
    std::fs::create_dir(&firmware_dir).expect("temp firmware dir should be created");

    let err = match load_backend_from_rom_source(
        ActiveSystem::Nes,
        &PathBuf::from("test.fds"),
        &PathBuf::from("test.fds"),
        Some(vec![
            0x55;
            zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE
        ]),
        BackendLoadConfig {
            firmware_search_dirs: vec![firmware_dir.clone()],
            ..BackendLoadConfig::default()
        },
    ) {
        Ok(_) => panic!("empty firmware directory should not satisfy FDS BIOS resolution"),
        Err(err) => err,
    };
    let _ = std::fs::remove_dir_all(&firmware_dir);

    let message = err.to_string();
    assert!(message.contains(&firmware_dir.display().to_string()));
    assert!(message.contains("No recognized nintendo.fds.bios"));
}

#[test]
fn shared_backend_loader_initializes_fds_with_resolved_bios() {
    static TEST_FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
        [0xFF; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];
    let fds_image = vec![0x55; zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE];
    let rom_path = PathBuf::from("test.fds");
    let loaded = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        Some(fds_image.clone()),
        BackendLoadConfig {
            fds_bios_override: Some(&TEST_FDS_BIOS),
            ..BackendLoadConfig::default()
        },
    )
    .expect("FDS app loader should initialize when BIOS bytes are resolved");

    assert_eq!(loaded.backend.system(), ActiveSystem::Nes);
    assert_eq!(loaded.backend.rom_path(), rom_path);
    assert_eq!(loaded.original_crc32, crc32fast::hash(&fds_image));
    assert_eq!(
        loaded.backend.save_ram_kind(),
        zeff_emu_common::save_ram::SaveRamKind::known_battery_backed(0x8000)
    );
    assert!(matches!(
        loaded.backend.replay_metadata().firmware.as_slice(),
        [zeff_emu_common::replay::ReplayFirmwareManifest::External {
            firmware_id,
            variant: Some(variant),
            sha256,
        }] if firmware_id == "nintendo.fds.bios"
            && variant == "test-override"
            && *sha256 == zeff_firmware::sha256_bytes(&TEST_FDS_BIOS)
    ));
    assert!(!loaded.backend.framebuffer().is_empty());
}

#[test]
fn shared_backend_loader_restores_fds_persistent_media_container() {
    static TEST_FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
        [0xFF; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];
    let mut fds_image = vec![0x55; zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE];
    fds_image[0] = 0x01;
    let seed = zeff_nes_core::emulator::Emulator::new_fds(
        &fds_image,
        TEST_FDS_BIOS.to_vec(),
        zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE,
    )
    .unwrap();
    let mut persistent = seed.dump_persistent_data().unwrap();
    *persistent.last_mut().unwrap() = 0xA7;

    let temp_dir = std::env::temp_dir().join(format!(
        "zeff_boy_fds_persistence_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let rom_path = temp_dir.join("media.fds");
    std::fs::write(rom_path.with_extension("sav"), &persistent).unwrap();

    let loaded = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        Some(fds_image),
        BackendLoadConfig {
            fds_bios_override: Some(&TEST_FDS_BIOS),
            ..BackendLoadConfig::default()
        },
    )
    .expect("FDS loader should restore its persistent media container");

    let EmuBackend::Nes(backend) = loaded.backend else {
        panic!("FDS content should use the NES backend");
    };
    assert_eq!(backend.emu.dump_persistent_data().unwrap(), persistent);
    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn system_specs_map_to_shared_backend_loader() {
    for spec in system_specs() {
        for extension in spec.rom_extensions {
            if *extension == "fds" {
                continue;
            }
            let rom = test_rom_for_system(spec.system);
            let rom_name = format!("matrix.{extension}");
            let rom_path = PathBuf::from(&rom_name);
            let result = load_backend_from_rom_source(
                spec.system,
                &rom_path,
                &rom_path,
                Some(rom),
                BackendLoadConfig {
                    sample_rate: Some(44_100),
                    coleco_bios_override: (spec.system == ActiveSystem::Coleco)
                        .then_some(&TEST_COLECO_BIOS),
                    ..BackendLoadConfig::default()
                },
            );
            if matches!(*extension, "cue" | "chd" | "iso") {
                let error = match result {
                    Ok(_) => panic!("packaged PC Engine CD media unexpectedly loaded"),
                    Err(error) => error,
                };
                assert!(error.to_string().contains("PackagedCdSetUnsupported"));
                continue;
            }
            let loaded = result.unwrap_or_else(|err| {
                panic!(
                    "shared backend loader should initialize {} ROM {rom_name}: {err}",
                    spec.code
                )
            });

            assert_eq!(loaded.backend.system(), spec.system);
            assert_eq!(loaded.backend.core_family(), spec.core_family);
            assert_eq!(loaded.backend.rom_path(), rom_path);
            assert_eq!(loaded.backend.source_path(), rom_path);
            assert_eq!(loaded.backend.framebuffer().len(), spec.framebuffer_len());
        }
    }
}

#[test]
fn active_system_firmware_plans_preserve_current_core_defaults() {
    assert!(firmware_plan_for_active_system(ActiveSystem::Nes).is_empty());
    assert!(firmware_plan_for_active_system(ActiveSystem::WonderSwan).is_empty());
    assert!(firmware_plan_for_active_system(ActiveSystem::Sg1000).is_empty());
    assert!(firmware_plan_for_active_system(ActiveSystem::Pce).is_empty());

    let gba_plan = firmware_plan_for_active_system(ActiveSystem::GameBoyAdvance);
    assert_eq!(gba_plan.len(), 1);
    assert_eq!(gba_plan[0].id.as_ref(), "nintendo.gba.bios");
    assert_eq!(
        gba_plan[0].requirement,
        zeff_firmware::RequirementLevel::Recommended
    );
    assert!(matches!(
        gba_plan[0].fallback,
        zeff_firmware::FallbackKind::Hle { .. }
    ));

    let sms_plan = firmware_plan_for_active_system(ActiveSystem::MasterSystem);
    assert_eq!(sms_plan.len(), 1);
    assert_eq!(sms_plan[0].id.as_ref(), "sega.sms.boot");
    assert!(matches!(
        sms_plan[0].fallback,
        zeff_firmware::FallbackKind::SkipBoot { .. }
    ));
}

#[test]
fn shared_backend_loader_records_default_firmware_manifests() {
    for system in [
        ActiveSystem::GameBoy,
        ActiveSystem::GameBoyAdvance,
        ActiveSystem::Nes,
        ActiveSystem::Pce,
        ActiveSystem::WonderSwan,
        ActiveSystem::MasterSystem,
        ActiveSystem::GameGear,
        ActiveSystem::Sg1000,
    ] {
        let path = PathBuf::from(match system {
            ActiveSystem::GameBoy => "firmware.gb",
            ActiveSystem::GameBoyAdvance => "firmware.gba",
            ActiveSystem::Nes => "firmware.nes",
            ActiveSystem::Coleco => "firmware.col",
            ActiveSystem::Pce => "firmware.pce",
            ActiveSystem::WonderSwan => "firmware.ws",
            ActiveSystem::MasterSystem => "firmware.sms",
            ActiveSystem::GameGear => "firmware.gg",
            ActiveSystem::Sg1000 => "firmware.sg",
        });
        let loaded = load_backend_from_rom_source(
            system,
            &path,
            &path,
            Some(test_rom_for_system(system)),
            BackendLoadConfig::default(),
        )
        .unwrap_or_else(|err| panic!("{system:?} firmware metadata load failed: {err}"));

        assert_eq!(
            loaded.backend.replay_metadata().firmware,
            default_firmware_manifests_for_active_system(system)
        );
    }

    assert!(matches!(
        default_firmware_manifests_for_active_system(ActiveSystem::GameBoyAdvance).as_slice(),
        [zeff_emu_common::replay::ReplayFirmwareManifest::Hle {
            firmware_id,
            implementation,
            compatibility_version: 1,
        }] if firmware_id == "nintendo.gba.bios" && implementation == "zeff-gba-hle"
    ));
}

#[test]
#[ignore = "requires ZEFF_FIRMWARE_TEST_DIR with a retail GBA BIOS"]
fn shared_gba_loader_uses_selected_external_bios() {
    let root = PathBuf::from(std::env::var("ZEFF_FIRMWARE_TEST_DIR").unwrap());
    let path = PathBuf::from("firmware-test.gba");
    let loaded = load_backend_from_rom_source(
        ActiveSystem::GameBoyAdvance,
        &path,
        &path,
        Some(build_gba_test_rom()),
        BackendLoadConfig {
            firmware_search_dirs: vec![root],
            gba_use_external_bios: true,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap();

    assert_eq!(loaded.backend.gba().unwrap().emu.cpu_pc(), 0);
    assert!(matches!(
        loaded.backend.replay_metadata().firmware.as_slice(),
        [zeff_emu_common::replay::ReplayFirmwareManifest::External {
            firmware_id,
            ..
        }] if firmware_id == "nintendo.gba.bios"
    ));
}

#[test]
#[ignore = "requires ZEFF_FIRMWARE_TEST_DIR with a recognized ColecoVision BIOS"]
fn shared_coleco_loader_uses_recognized_external_bios() {
    let root = PathBuf::from(std::env::var("ZEFF_FIRMWARE_TEST_DIR").unwrap());
    let path = PathBuf::from("firmware-test.col");
    let loaded = load_backend_from_rom_source(
        ActiveSystem::Coleco,
        &path,
        &path,
        Some(test_rom_for_system(ActiveSystem::Coleco)),
        BackendLoadConfig {
            firmware_search_dirs: vec![root],
            ..BackendLoadConfig::default()
        },
    )
    .unwrap();

    let EmuBackend::Coleco(backend) = &loaded.backend else {
        panic!("expected ColecoVision backend");
    };
    assert_eq!(backend.emu.cpu().regs().pc, 0);
    assert!(matches!(
        loaded.backend.replay_metadata().firmware.as_slice(),
        [zeff_emu_common::replay::ReplayFirmwareManifest::External { firmware_id, .. }]
            if firmware_id == "coleco.vision.bios"
    ));
}

#[test]
#[ignore = "requires ZEFF_FIRMWARE_TEST_DIR with retail GB boot ROMs"]
fn shared_gb_loader_uses_boot_rom_for_selected_hardware() {
    let root = PathBuf::from(std::env::var("ZEFF_FIRMWARE_TEST_DIR").unwrap());
    let path = PathBuf::from("firmware-test.gbc");
    let mut rom = test_rom_for_system(ActiveSystem::GameBoy);
    rom[0x143] = 0x80;
    let loaded = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &path,
        &path,
        Some(rom),
        BackendLoadConfig {
            firmware_search_dirs: vec![root],
            gb_use_external_boot_rom: true,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap();

    let emu = &loaded.backend.gb().unwrap().emu;
    assert_eq!(emu.cpu_pc(), 0);
    assert!(emu.boot_rom_enabled());
    assert!(
        loaded
            .backend
            .replay_metadata()
            .firmware
            .iter()
            .any(|firmware| {
                matches!(
                    firmware,
                    zeff_emu_common::replay::ReplayFirmwareManifest::External { firmware_id, .. }
                        if firmware_id == "nintendo.gb.boot.cgb"
                )
            })
    );
}

#[test]
#[ignore = "requires ZEFF_FIRMWARE_TEST_DIR with retail Sega boot ROMs"]
fn shared_sega8_loader_uses_selected_boot_rom() {
    let root = PathBuf::from(std::env::var("ZEFF_FIRMWARE_TEST_DIR").unwrap());
    for (system, name, firmware_id) in [
        (
            ActiveSystem::MasterSystem,
            "firmware-test.sms",
            "sega.sms.boot",
        ),
        (ActiveSystem::GameGear, "firmware-test.gg", "sega.gg.boot"),
    ] {
        let path = PathBuf::from(name);
        let loaded = load_backend_from_rom_source(
            system,
            &path,
            &path,
            Some(test_rom_for_system(system)),
            BackendLoadConfig {
                firmware_search_dirs: vec![root.clone()],
                sega8_use_external_boot_rom: true,
                ..BackendLoadConfig::default()
            },
        )
        .unwrap();

        assert!(loaded.backend.sega8().unwrap().emu.bus().boot_rom_enabled());
        assert!(matches!(
            loaded.backend.replay_metadata().firmware.as_slice(),
            [zeff_emu_common::replay::ReplayFirmwareManifest::External {
                firmware_id: actual,
                ..
            }] if actual == firmware_id
        ));
    }
}

#[test]
fn shared_backend_loader_preserves_archive_source_path() {
    let rom = build_gba_test_rom();
    let original_crc = crc32fast::hash(&rom);
    let source_path = PathBuf::from("archive.zip");
    let rom_path = PathBuf::from("inside_archive.gba");
    let loaded = load_backend_from_rom_source(
        ActiveSystem::GameBoyAdvance,
        &source_path,
        &rom_path,
        Some(rom),
        BackendLoadConfig::default(),
    )
    .expect("shared backend loader should initialize archived test ROM");

    assert_eq!(loaded.original_crc32, original_crc);
    assert_eq!(loaded.backend.rom_path(), rom_path);
    assert_eq!(loaded.backend.source_path(), source_path);
}

#[test]
fn shared_backend_loader_applies_explicit_sega8_mapper_tag_from_paths() {
    let rom = build_sms_test_rom();
    let loaded = load_backend_from_rom_source(
        ActiveSystem::MasterSystem,
        &PathBuf::from("archive [mapper=janggun].zip"),
        &PathBuf::from("inside.sms"),
        Some(rom),
        BackendLoadConfig::default(),
    )
    .expect("shared backend loader should initialize tagged Sega 8-bit ROM");

    let sega8 = loaded
        .backend
        .sega8()
        .expect("loaded backend should be Sega 8-bit");
    assert_eq!(
        sega8.emu.bus().mapper().kind(),
        zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Janggun
    );
}

fn test_rom_for_system(system: ActiveSystem) -> Vec<u8> {
    match system {
        ActiveSystem::GameBoy => build_gb_test_rom(),
        ActiveSystem::GameBoyAdvance => build_gba_test_rom(),
        ActiveSystem::Nes => build_nes_test_rom(),
        ActiveSystem::Coleco => {
            let mut rom = vec![0; 8 * 1024];
            rom[..2].copy_from_slice(&[0xAA, 0x55]);
            rom
        }
        ActiveSystem::Pce => build_pce_test_rom(),
        ActiveSystem::WonderSwan => build_ws_test_rom(),
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => {
            build_sms_test_rom()
        }
    }
}

use super::*;
use crate::emu_backend::loader::{
    DirectGameGearTasExecutionLoader, register_test_game_gear_board_catalog_entry,
};
use zeff_sega8_core::hardware::cartridge::{
    GameGearCartridgeIdentity, GameGearStandardMapperRam,
    game_gear_standard_mapper_ram_identity_from_catalog_entry,
};

impl BatteryRepairHarness {
    fn game_gear_direct(label: &str, seed: u8) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.gg");
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = game_gear_battery_rom(seed);
        let identity = game_gear_identity(&rom);
        let catalog = register_test_game_gear_board_catalog_entry(
            identity,
            GameGearStandardMapperRam::BatteryBacked8KiB,
        );
        let project_sram = game_gear_save_bytes(seed.wrapping_add(0x31));
        std::fs::write(&source_path, &rom).unwrap();
        std::fs::write(&save_path, &project_sram).unwrap();
        let project = DirectGameGearTasExecutionLoader::new(source_path.clone())
            .create_project_file(&project_path)
            .unwrap();
        std::fs::write(&save_path, game_gear_save_bytes(seed.wrapping_add(0xA7))).unwrap();
        Self::finish_game_gear(
            root,
            BatteryRepairPaths {
                source: source_path.clone(),
                rom: source_path,
                project: project_path,
                save: save_path,
            },
            None,
            (identity, project.identity().sync_config_sha256),
            project_sram,
            catalog,
        )
    }

    fn game_gear_zip(label: &str, seed: u8) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.zip");
        let member_name = "folder/battery.gg";
        let rom_path = source_path.join(member_name);
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = game_gear_battery_rom(seed);
        let identity = game_gear_identity(&rom);
        let catalog = register_test_game_gear_board_catalog_entry(
            identity,
            GameGearStandardMapperRam::BatteryBacked8KiB,
        );
        let project_sram = game_gear_save_bytes(seed.wrapping_add(0x47));
        write_zip(&source_path, &[(member_name, &rom)]).unwrap();
        std::fs::write(&save_path, &project_sram).unwrap();
        let project = DirectGameGearTasExecutionLoader::new_zip(
            source_path.clone(),
            Some(rom_path.clone()),
            false,
        )
        .create_project_file(&project_path)
        .unwrap();
        std::fs::write(&save_path, game_gear_save_bytes(seed.wrapping_add(0xC9))).unwrap();
        Self::finish_game_gear(
            root,
            BatteryRepairPaths {
                source: source_path,
                rom: rom_path,
                project: project_path,
                save: save_path,
            },
            Some(rom),
            (identity, project.identity().sync_config_sha256),
            project_sram,
            catalog,
        )
    }

    fn finish_game_gear(
        root: crate::test_support::TestDirectory,
        paths: BatteryRepairPaths,
        preloaded_rom: Option<Vec<u8>>,
        identity: (GameGearCartridgeIdentity, TasDigest),
        project_sram: Vec<u8>,
        catalog: crate::emu_backend::loader::TestGameGearBoardCatalogGuard,
    ) -> Self {
        let mut original_backend = load_backend_from_rom_source(
            ActiveSystem::GameGear,
            &paths.source,
            &paths.rom,
            preloaded_rom,
            BackendLoadConfig {
                sample_rate: None,
                apply_mods: false,
                initial_input: None,
                game_gear_standard_mapper_ram_identity: Some(
                    game_gear_standard_mapper_ram_identity_from_catalog_entry(
                        identity.0,
                        GameGearStandardMapperRam::BatteryBacked8KiB,
                    ),
                ),
                sega8_load_battery_sram: true,
                sega8_video_standard: Some(
                    zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc,
                ),
                sega8_console_region: Some(zeff_sega8_core::hardware::region::Sega8Region::Export),
                sega8_use_external_boot_rom: false,
                ..BackendLoadConfig::default()
            },
        )
        .unwrap()
        .backend;
        let EmuBackend::Sega8(sega8) = &mut original_backend else {
            unreachable!();
        };
        sega8.set_game_gear_tas_sync_config_sha256(identity.1.0);
        assert!(
            crate::emu_thread::build_tas_repair_witness(
                &original_backend,
                TasExecutionProfile::DirectGameGearCartridge,
            )
            .is_err()
        );
        let original_state = original_backend.encode_state_bytes().unwrap();
        let rom_sha256 = original_backend.rom_hash();
        let worker = EmuThread::spawn(original_backend, false);
        let mut app = app_with_worker(
            worker,
            ORIGINAL_GENERATION,
            ActiveSystem::GameGear,
            paths.rom.clone(),
        );
        app.rom_info.source_path = Some(paths.source);
        app.rom_info.rom_path = Some(paths.rom);
        let generation_path = root.path().join("battery-generation.bin");
        let recovery_state_path = root.path().join("recovery-state.zst");
        app.tas_repair
            .set_repaired_recovery_for_test(crate::emu_thread::RecoveryTestConfig {
                generation_path: generation_path.clone(),
                state_path: recovery_state_path.clone(),
                fail_generation_write: false,
            });
        let opened = live_ok(
            &mut app,
            LiveCommand::TasOpenProject {
                path: paths.project,
            },
        );
        assert_eq!(opened["project"]["frame_count"], 1);
        wait_for_readiness(&mut app, "reload_required");
        live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 1 });
        Self {
            app,
            _root: root,
            save_path: paths.save,
            project_sram,
            original_state,
            rom_sha256,
            generation_path,
            recovery_state_path,
            _game_gear_catalog: Some(catalog),
        }
    }
}

#[test]
fn direct_game_gear_restore_preserves_the_sidecar_and_original() {
    assert_battery_restore(BatteryRepairHarness::game_gear_direct(
        "tas-repair-game-gear-direct-restore",
        0x11,
    ));
}

#[test]
fn zip_game_gear_restore_preserves_the_outer_sidecar_and_original() {
    assert_battery_restore(BatteryRepairHarness::game_gear_zip(
        "tas-repair-game-gear-zip-restore",
        0x12,
    ));
}

#[test]
fn direct_game_gear_keep_publishes_the_project_candidate() {
    assert_battery_keep(
        BatteryRepairHarness::game_gear_direct("tas-repair-game-gear-direct-keep", 0x13),
        true,
    );
}

#[test]
fn zip_game_gear_keep_publishes_to_the_outer_sidecar() {
    assert_battery_keep(
        BatteryRepairHarness::game_gear_zip("tas-repair-game-gear-zip-keep", 0x14),
        true,
    );
}

#[test]
fn direct_game_gear_uncertain_publication_keeps_repaired_ownership() {
    assert_uncertain_keep(BatteryRepairHarness::game_gear_direct(
        "tas-repair-game-gear-direct-uncertain",
        0x15,
    ));
}

#[test]
fn zip_game_gear_uncertain_publication_keeps_repaired_ownership() {
    assert_uncertain_keep(BatteryRepairHarness::game_gear_zip(
        "tas-repair-game-gear-zip-uncertain",
        0x16,
    ));
}

#[test]
fn direct_game_gear_cas_conflict_preserves_restore_authority() {
    assert_cas_conflict(
        BatteryRepairHarness::game_gear_direct("tas-repair-game-gear-direct-conflict", 0x17),
        0xE7,
    );
}

#[test]
fn zip_game_gear_cas_conflict_preserves_restore_authority() {
    assert_cas_conflict(
        BatteryRepairHarness::game_gear_zip("tas-repair-game-gear-zip-conflict", 0x18),
        0xE7,
    );
}

#[test]
fn game_gear_battery_rejects_non_8kib_sidecars() {
    let root = crate::test_support::test_directory("tas-repair-game-gear-wrong-save").unwrap();
    let source_path = root.path().join("battery.gg");
    let rom = game_gear_battery_rom(0x19);
    let identity = game_gear_identity(&rom);
    let _catalog = register_test_game_gear_board_catalog_entry(
        identity,
        GameGearStandardMapperRam::BatteryBacked8KiB,
    );
    std::fs::write(&source_path, rom).unwrap();
    std::fs::write(source_path.with_extension("sav"), vec![0x5C; 8 * 1024 - 1]).unwrap();
    assert!(
        DirectGameGearTasExecutionLoader::new(source_path)
            .create_project()
            .is_err()
    );
}

#[test]
fn game_gear_battery_catalog_evidence_requires_exact_media_identity() {
    let root = crate::test_support::test_directory("tas-repair-game-gear-wrong-media").unwrap();
    let source_path = root.path().join("battery.gg");
    let catalogued = game_gear_battery_rom(0x1A);
    let _catalog = register_test_game_gear_board_catalog_entry(
        game_gear_identity(&catalogued),
        GameGearStandardMapperRam::BatteryBacked8KiB,
    );
    std::fs::write(&source_path, game_gear_battery_rom(0x1B)).unwrap();
    std::fs::write(
        source_path.with_extension("sav"),
        game_gear_save_bytes(0x81),
    )
    .unwrap();
    assert!(
        DirectGameGearTasExecutionLoader::new(source_path)
            .create_project()
            .is_err()
    );
}

fn game_gear_battery_rom(seed: u8) -> Vec<u8> {
    let mut rom = vec![seed; 16 * 1024];
    let offset = 0x3FF0;
    rom[offset..offset + 8].copy_from_slice(b"TMR SEGA");
    rom[offset + 0x0A..offset + 0x0C].copy_from_slice(&0x1234u16.to_le_bytes());
    rom[offset + 0x0C] = seed;
    rom[offset + 0x0D] = 0x31;
    rom[offset + 0x0E] = 0xA5;
    rom[offset + 0x0F] = 0x6A;
    rom
}

fn game_gear_identity(rom: &[u8]) -> GameGearCartridgeIdentity {
    GameGearCartridgeIdentity {
        sha256: zeff_firmware::sha256_bytes(rom),
        source_len: rom.len(),
    }
}

fn game_gear_save_bytes(seed: u8) -> Vec<u8> {
    (0..8 * 1024)
        .map(|index| (index as u8).wrapping_mul(37).wrapping_add(seed))
        .collect()
}

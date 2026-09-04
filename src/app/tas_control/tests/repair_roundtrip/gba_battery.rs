use super::*;
use crate::emu_backend::loader::DirectGbaTasExecutionLoader;

impl BatteryRepairHarness {
    fn gba_direct(label: &str, marker: &[u8]) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.gba");
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = gba_battery_test_rom(marker);
        let project_save = gba_save_bytes(marker, 0x43);
        std::fs::write(&source_path, &rom).unwrap();
        std::fs::write(&save_path, &project_save).unwrap();
        DirectGbaTasExecutionLoader::new(source_path.clone())
            .create_project_file(&project_path)
            .unwrap();
        std::fs::write(&save_path, gba_save_bytes(marker, 0xB8)).unwrap();
        Self::finish_gba(
            root,
            BatteryRepairPaths {
                source: source_path.clone(),
                rom: source_path,
                project: project_path,
                save: save_path,
            },
            None,
            project_save,
        )
    }

    fn gba_zip(label: &str, marker: &[u8]) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.zip");
        let member_name = "folder/battery.gba";
        let rom_path = source_path.join(member_name);
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = gba_battery_test_rom(marker);
        let project_save = gba_save_bytes(marker, 0x79);
        write_zip(&source_path, &[(member_name, &rom)]).unwrap();
        std::fs::write(&save_path, &project_save).unwrap();
        DirectGbaTasExecutionLoader::new_zip(source_path.clone(), Some(rom_path.clone()))
            .create_project_file(&project_path)
            .unwrap();
        std::fs::write(&save_path, gba_save_bytes(marker, 0xCE)).unwrap();
        Self::finish_gba(
            root,
            BatteryRepairPaths {
                source: source_path,
                rom: rom_path,
                project: project_path,
                save: save_path,
            },
            Some(rom),
            project_save,
        )
    }

    fn finish_gba(
        root: crate::test_support::TestDirectory,
        paths: BatteryRepairPaths,
        preloaded_rom: Option<Vec<u8>>,
        project_save: Vec<u8>,
    ) -> Self {
        let original_backend = load_backend_from_rom_source(
            ActiveSystem::GameBoyAdvance,
            &paths.source,
            &paths.rom,
            preloaded_rom,
            BackendLoadConfig {
                sample_rate: Some(crate::emu_backend::gba::DIRECT_GBA_SAMPLE_RATE),
                apply_mods: false,
                initial_input: Some((0, 0)),
                gba_load_battery_sram: true,
                gba_use_external_bios: false,
                ..BackendLoadConfig::default()
            },
        )
        .unwrap()
        .backend;
        assert!(
            crate::emu_thread::build_tas_repair_witness(
                &original_backend,
                TasExecutionProfile::DirectGbaCartridge,
            )
            .is_err()
        );
        let original_state = original_backend.encode_state_bytes().unwrap();
        let rom_sha256 = original_backend.rom_hash();
        let worker = EmuThread::spawn(original_backend, false);
        let mut app = app_with_worker(
            worker,
            ORIGINAL_GENERATION,
            ActiveSystem::GameBoyAdvance,
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
            project_sram: project_save,
            original_state,
            rom_sha256,
            generation_path,
            recovery_state_path,
            _game_gear_catalog: None,
        }
    }
}

#[test]
fn direct_gba_sram_restore_preserves_the_sidecar_and_original() {
    assert_battery_restore(BatteryRepairHarness::gba_direct(
        "tas-repair-gba-sram-direct-restore",
        b"SRAM_V113",
    ));
}

#[test]
fn zip_gba_flash512_restore_preserves_the_sidecar_and_original() {
    assert_battery_restore(BatteryRepairHarness::gba_zip(
        "tas-repair-gba-flash512-zip-restore",
        b"FLASH512_V131",
    ));
}

#[test]
fn direct_gba_flash1m_keep_publishes_the_project_candidate() {
    assert_battery_keep(
        BatteryRepairHarness::gba_direct("tas-repair-gba-flash1m-direct-keep", b"FLASH1M_V103"),
        true,
    );
}

#[test]
fn zip_gba_eeprom_keep_publishes_to_the_archive_sidecar() {
    assert_battery_keep(
        BatteryRepairHarness::gba_zip("tas-repair-gba-eeprom-zip-keep", b"EEPROM_V122"),
        true,
    );
}

#[test]
fn direct_gba_uncertain_publication_keeps_repaired_ownership() {
    assert_uncertain_keep(BatteryRepairHarness::gba_direct(
        "tas-repair-gba-flash512-direct-uncertain",
        b"FLASH512_V131",
    ));
}

#[test]
fn zip_gba_uncertain_publication_keeps_repaired_ownership() {
    assert_uncertain_keep(BatteryRepairHarness::gba_zip(
        "tas-repair-gba-flash1m-zip-uncertain",
        b"FLASH1M_V103",
    ));
}

#[test]
fn direct_gba_cas_conflict_preserves_restore_authority() {
    assert_cas_conflict(
        BatteryRepairHarness::gba_direct("tas-repair-gba-eeprom-direct-conflict", b"EEPROM_V122"),
        0xE4,
    );
}

#[test]
fn zip_gba_cas_conflict_preserves_restore_authority() {
    assert_cas_conflict(
        BatteryRepairHarness::gba_zip("tas-repair-gba-sram-zip-conflict", b"SRAM_V113"),
        0xE4,
    );
}

fn gba_battery_test_rom(marker: &[u8]) -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom.extend_from_slice(marker);
    rom
}

fn gba_save_bytes(marker: &[u8], seed: u8) -> Vec<u8> {
    let len = match marker {
        b"SRAM_V113" | b"FLASH512_V131" => 0x10000,
        b"FLASH1M_V103" => 0x20000,
        b"EEPROM_V122" => 0x2000,
        _ => panic!("unsupported synthetic GBA backup marker"),
    };
    (0..len)
        .map(|index| (index as u8).wrapping_mul(29).wrapping_add(seed))
        .collect()
}

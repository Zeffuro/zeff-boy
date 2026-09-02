use super::*;
use crate::emu_backend::loader::DirectWsTasExecutionLoader;
use zeff_ws_core::hardware::cartridge::{RomOrientation, compute_footer_checksum};

impl BatteryRepairHarness {
    fn ws_direct(label: &str) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.ws");
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = ws_battery_test_rom(0, RomOrientation::Horizontal, 0x03);
        let project_sram = vec![0x53; 128 * 1024];
        std::fs::write(&source_path, &rom).unwrap();
        std::fs::write(&save_path, &project_sram).unwrap();
        DirectWsTasExecutionLoader::new(source_path.clone())
            .create_project_file(&project_path)
            .unwrap();
        std::fs::write(&save_path, vec![0xC4; project_sram.len()]).unwrap();
        Self::finish_ws(
            root,
            BatteryRepairPaths {
                source: source_path.clone(),
                rom: source_path,
                project: project_path,
                save: save_path,
            },
            None,
            project_sram,
        )
    }

    fn ws_zip(label: &str) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.zip");
        let member_name = "folder/battery.wsc";
        let rom_path = source_path.join(member_name);
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = ws_battery_test_rom(1, RomOrientation::Vertical, 0x20);
        let project_sram = vec![0x86; 1024];
        write_zip(&source_path, &[(member_name, &rom)]).unwrap();
        std::fs::write(&save_path, &project_sram).unwrap();
        DirectWsTasExecutionLoader::new_zip(source_path.clone(), Some(rom_path.clone()))
            .create_project_file(&project_path)
            .unwrap();
        std::fs::write(&save_path, vec![0x39; project_sram.len()]).unwrap();
        Self::finish_ws(
            root,
            BatteryRepairPaths {
                source: source_path,
                rom: rom_path,
                project: project_path,
                save: save_path,
            },
            Some(rom),
            project_sram,
        )
    }

    fn finish_ws(
        root: crate::test_support::TestDirectory,
        paths: BatteryRepairPaths,
        preloaded_rom: Option<Vec<u8>>,
        project_sram: Vec<u8>,
    ) -> Self {
        let original_backend = load_backend_from_rom_source(
            ActiveSystem::WonderSwan,
            &paths.source,
            &paths.rom,
            preloaded_rom,
            BackendLoadConfig {
                apply_mods: false,
                ws_load_battery_sram: true,
                ..BackendLoadConfig::default()
            },
        )
        .unwrap()
        .backend;
        let original_state = original_backend.encode_state_bytes().unwrap();
        let rom_sha256 = original_backend.rom_hash();
        let worker = EmuThread::spawn(original_backend, false);
        let mut app = app_with_worker(
            worker,
            ORIGINAL_GENERATION,
            ActiveSystem::WonderSwan,
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
            _game_gear_catalog: None,
        }
    }
}

#[test]
fn direct_ws_battery_restore_preserves_the_sidecar_and_original() {
    assert_battery_restore(BatteryRepairHarness::ws_direct(
        "tas-repair-ws-battery-direct-restore",
    ));
}

#[test]
fn zip_ws_battery_restore_preserves_the_archive_sidecar_and_original() {
    assert_battery_restore(BatteryRepairHarness::ws_zip(
        "tas-repair-ws-battery-zip-restore",
    ));
}

#[test]
fn direct_ws_battery_keep_is_durable() {
    assert_battery_keep(
        BatteryRepairHarness::ws_direct("tas-repair-ws-battery-direct-keep"),
        true,
    );
}

#[test]
fn zip_ws_battery_keep_publishes_to_the_outer_archive_sidecar() {
    assert_battery_keep(
        BatteryRepairHarness::ws_zip("tas-repair-ws-battery-zip-keep"),
        true,
    );
}

#[test]
fn direct_ws_uncertain_publication_keeps_repaired_ownership() {
    assert_uncertain_keep(BatteryRepairHarness::ws_direct(
        "tas-repair-ws-battery-direct-uncertain",
    ));
}

#[test]
fn zip_ws_uncertain_publication_keeps_repaired_ownership() {
    assert_uncertain_keep(BatteryRepairHarness::ws_zip(
        "tas-repair-ws-battery-zip-uncertain",
    ));
}

#[test]
fn direct_ws_cas_conflict_preserves_restore_authority() {
    assert_cas_conflict(
        BatteryRepairHarness::ws_direct("tas-repair-ws-battery-direct-conflict"),
        0xE7,
    );
}

#[test]
fn zip_ws_cas_conflict_preserves_restore_authority() {
    assert_cas_conflict(
        BatteryRepairHarness::ws_zip("tas-repair-ws-battery-zip-conflict"),
        0xE7,
    );
}

#[test]
fn ws_rtc_battery_uses_the_aggregate_repair_contract() {
    let root = crate::test_support::test_directory("tas-repair-ws-rtc-battery-rejected").unwrap();
    let source_path = root.path().join("clock.ws");
    let mut rom = ws_battery_test_rom(0, RomOrientation::Horizontal, 0x03);
    let footer = rom.len() - 10;
    rom[footer + 7] = 1;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    std::fs::write(&source_path, rom).unwrap();
    std::fs::write(source_path.with_extension("sav"), [0x51; 128 * 1024]).unwrap();
    let loader = DirectWsTasExecutionLoader::new(source_path);
    let project = loader.create_project().unwrap();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();

    let contract = crate::app::tas_control::repair::persistence_contract_for_project(
        &project,
        &backend,
        crate::emu_thread::TasExecutionProfile::DirectWsCartridge,
    )
    .unwrap();
    assert!(matches!(
        contract,
        crate::emu_thread::TasPersistenceContract::WsRtcBattery {
            byte_len,
            ..
        } if byte_len == 128 * 1024 + 24
    ));
}

fn ws_battery_test_rom(system: u8, orientation: RomOrientation, save_kind: u8) -> Vec<u8> {
    let mut rom = vec![0x90; 128 * 1024];
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer..].fill(0);
    rom[footer + 1] = system;
    rom[footer + 4] = 0x01;
    rom[footer + 5] = save_kind;
    rom[footer + 6] = u8::from(orientation == RomOrientation::Vertical);
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

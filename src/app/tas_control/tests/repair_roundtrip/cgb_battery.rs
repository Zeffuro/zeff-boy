use super::*;
use crate::emu_backend::loader::DirectGbcTasExecutionLoader;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

impl BatteryRepairHarness {
    fn cgb_direct(label: &str) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.gbc");
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = cgb_battery_test_rom();
        let project_sram = vec![0x4B; 32 * 1024];
        std::fs::write(&source_path, &rom).unwrap();
        std::fs::write(&save_path, &project_sram).unwrap();
        DirectGbcTasExecutionLoader::new(source_path.clone(), Vec::new())
            .create_project_file(&project_path)
            .unwrap();
        std::fs::write(&save_path, vec![0xD2; project_sram.len()]).unwrap();
        Self::finish_gb(
            root,
            BatteryRepairPaths {
                source: source_path.clone(),
                rom: source_path,
                project: project_path,
                save: save_path,
            },
            None,
            project_sram,
            None,
            HardwareModePreference::ForceCgb,
        )
    }

    fn cgb_zip(label: &str) -> Self {
        let root = crate::test_support::test_directory(label).unwrap();
        let source_path = root.path().join("battery.zip");
        let member_name = "folder/battery.gbc";
        let rom_path = source_path.join(member_name);
        let save_path = source_path.with_extension("sav");
        let project_path = root.path().join("battery.ztas");
        let rom = cgb_battery_test_rom();
        let project_sram = vec![0x87; 32 * 1024];
        write_zip(&source_path, &[(member_name, &rom)]).unwrap();
        std::fs::write(&save_path, &project_sram).unwrap();
        DirectGbcTasExecutionLoader::new_zip(
            source_path.clone(),
            Some(rom_path.clone()),
            Vec::new(),
        )
        .create_project_file(&project_path)
        .unwrap();
        std::fs::write(&save_path, vec![0x39; project_sram.len()]).unwrap();
        let archive = std::fs::read(&source_path).unwrap();
        Self::finish_gb(
            root,
            BatteryRepairPaths {
                source: source_path,
                rom: rom_path,
                project: project_path,
                save: save_path,
            },
            Some(rom),
            project_sram,
            Some((
                TasDigest::from_bytes(&archive).0,
                archive.len(),
                crate::emu_backend::loader::zip_gbc_battery_tas_sync_config_sha256(member_name).0,
            )),
            HardwareModePreference::ForceCgb,
        )
    }
}

#[test]
fn direct_cgb_battery_restore_preserves_the_sidecar_and_original() {
    assert_battery_restore(BatteryRepairHarness::cgb_direct(
        "tas-repair-cgb-battery-direct-restore",
    ));
}

#[test]
fn zip_cgb_battery_restore_preserves_the_archive_sidecar_and_original() {
    assert_battery_restore(BatteryRepairHarness::cgb_zip(
        "tas-repair-cgb-battery-zip-restore",
    ));
}

#[test]
fn direct_cgb_battery_keep_publishes_the_project_candidate() {
    assert_battery_keep(
        BatteryRepairHarness::cgb_direct("tas-repair-cgb-battery-direct-keep"),
        true,
    );
}

#[test]
fn zip_cgb_battery_keep_publishes_to_the_archive_sidecar() {
    assert_battery_keep(
        BatteryRepairHarness::cgb_zip("tas-repair-cgb-battery-zip-keep"),
        true,
    );
}

#[test]
fn direct_cgb_uncertain_publication_keeps_repaired_ownership() {
    assert_uncertain_keep(BatteryRepairHarness::cgb_direct(
        "tas-repair-cgb-battery-direct-uncertain",
    ));
}

#[test]
fn zip_cgb_uncertain_publication_keeps_repaired_ownership() {
    assert_uncertain_keep(BatteryRepairHarness::cgb_zip(
        "tas-repair-cgb-battery-zip-uncertain",
    ));
}

#[test]
fn direct_cgb_cas_conflict_preserves_restore_authority() {
    assert_cas_conflict(
        BatteryRepairHarness::cgb_direct("tas-repair-cgb-battery-direct-conflict"),
        0xE7,
    );
}

#[test]
fn zip_cgb_cas_conflict_preserves_restore_authority() {
    assert_cas_conflict(
        BatteryRepairHarness::cgb_zip("tas-repair-cgb-battery-zip-conflict"),
        0xE7,
    );
}

fn cgb_battery_test_rom() -> Vec<u8> {
    let mut rom = gb_battery_test_rom();
    rom[0x143] = 0xC0;
    rom
}

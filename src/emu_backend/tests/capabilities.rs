use super::*;

#[test]
fn backend_feature_contract_covers_every_supported_core() {
    assert_backend_feature_contract(
        build_gb_backend(),
        ActiveSystem::GameBoy,
        SaveRamKind::none(),
        zeff_gb_core::hardware::types::constants::WRAM_SIZE * 8,
        zeff_gb_core::hardware::types::constants::VRAM_SIZE * 2,
    );
    assert_backend_feature_contract(
        build_gba_backend(),
        ActiveSystem::GameBoyAdvance,
        SaveRamKind::none(),
        zeff_gba_core::hardware::constants::EWRAM_SIZE
            + zeff_gba_core::hardware::constants::IWRAM_SIZE,
        zeff_gba_core::hardware::constants::VRAM_SIZE,
    );
    assert_backend_feature_contract(
        build_nes_backend(),
        ActiveSystem::Nes,
        SaveRamKind::none(),
        0x800,
        0x2000,
    );
    assert_backend_feature_contract(
        build_coleco_backend(),
        ActiveSystem::Coleco,
        SaveRamKind::none(),
        zeff_coleco_core::constants::WORK_RAM_SIZE,
        zeff_coleco_core::constants::VRAM_SIZE,
    );
    assert_backend_feature_contract(
        build_ws_backend(),
        ActiveSystem::WonderSwan,
        SaveRamKind::none(),
        zeff_ws_core::hardware::constants::WS_INTERNAL_RAM_SIZE,
        zeff_ws_core::hardware::constants::WS_INTERNAL_RAM_SIZE,
    );
    assert_backend_feature_contract(
        build_sms_backend(),
        ActiveSystem::MasterSystem,
        SaveRamKind::mapper_ram_unknown(
            zeff_sega8_core::hardware::constants::SMS_CARTRIDGE_RAM_SIZE,
        ),
        zeff_sega8_core::hardware::constants::SMS_WORK_RAM_SIZE,
        zeff_sega8_core::hardware::constants::SMS_VRAM_SIZE,
    );
    assert_backend_feature_contract(
        load_test_backend_with_shared_loader(
            ActiveSystem::GameGear,
            "test.gg",
            build_sms_test_rom(),
        ),
        ActiveSystem::GameGear,
        SaveRamKind::mapper_ram_unknown(
            zeff_sega8_core::hardware::constants::SMS_CARTRIDGE_RAM_SIZE,
        ),
        zeff_sega8_core::hardware::constants::SMS_WORK_RAM_SIZE,
        zeff_sega8_core::hardware::constants::SMS_VRAM_SIZE,
    );
    assert_backend_feature_contract(
        load_test_backend_with_shared_loader(ActiveSystem::Sg1000, "test.sg", build_sms_test_rom()),
        ActiveSystem::Sg1000,
        SaveRamKind::mapper_ram_unknown(
            zeff_sega8_core::hardware::constants::SMS_CARTRIDGE_RAM_SIZE,
        ),
        zeff_sega8_core::hardware::constants::SG_WORK_RAM_SIZE,
        zeff_sega8_core::hardware::constants::SMS_VRAM_SIZE,
    );
    assert_backend_feature_contract(
        build_pce_backend(),
        ActiveSystem::Pce,
        SaveRamKind::none(),
        zeff_pce_core::hardware::WORK_RAM_LEN,
        zeff_pce_core::hardware::VDC_VRAM_BYTES,
    );
}

#[test]
fn app_ui_snapshot_reports_core_features_for_every_supported_core() {
    assert_app_snapshot_core_features(
        build_gb_backend(),
        SaveRamKind::none(),
        zeff_gb_core::hardware::types::constants::WRAM_SIZE * 8,
        zeff_gb_core::hardware::types::constants::VRAM_SIZE * 2,
    );
    assert_app_snapshot_core_features(
        build_gba_backend(),
        SaveRamKind::none(),
        zeff_gba_core::hardware::constants::EWRAM_SIZE
            + zeff_gba_core::hardware::constants::IWRAM_SIZE,
        zeff_gba_core::hardware::constants::VRAM_SIZE,
    );
    assert_app_snapshot_core_features(build_nes_backend(), SaveRamKind::none(), 0x800, 0x2000);
    assert_app_snapshot_core_features(
        build_coleco_backend(),
        SaveRamKind::none(),
        zeff_coleco_core::constants::WORK_RAM_SIZE,
        zeff_coleco_core::constants::VRAM_SIZE,
    );
    assert_app_snapshot_core_features(
        build_ws_backend(),
        SaveRamKind::none(),
        zeff_ws_core::hardware::constants::WS_INTERNAL_RAM_SIZE,
        zeff_ws_core::hardware::constants::WS_INTERNAL_RAM_SIZE,
    );
    assert_app_snapshot_core_features(
        build_sms_backend(),
        SaveRamKind::mapper_ram_unknown(
            zeff_sega8_core::hardware::constants::SMS_CARTRIDGE_RAM_SIZE,
        ),
        zeff_sega8_core::hardware::constants::SMS_WORK_RAM_SIZE,
        zeff_sega8_core::hardware::constants::SMS_VRAM_SIZE,
    );
    assert_app_snapshot_core_features(
        load_test_backend_with_shared_loader(
            ActiveSystem::GameGear,
            "test.gg",
            build_sms_test_rom(),
        ),
        SaveRamKind::mapper_ram_unknown(
            zeff_sega8_core::hardware::constants::SMS_CARTRIDGE_RAM_SIZE,
        ),
        zeff_sega8_core::hardware::constants::SMS_WORK_RAM_SIZE,
        zeff_sega8_core::hardware::constants::SMS_VRAM_SIZE,
    );
    assert_app_snapshot_core_features(
        load_test_backend_with_shared_loader(ActiveSystem::Sg1000, "test.sg", build_sms_test_rom()),
        SaveRamKind::mapper_ram_unknown(
            zeff_sega8_core::hardware::constants::SMS_CARTRIDGE_RAM_SIZE,
        ),
        zeff_sega8_core::hardware::constants::SG_WORK_RAM_SIZE,
        zeff_sega8_core::hardware::constants::SMS_VRAM_SIZE,
    );
    assert_app_snapshot_core_features(
        build_pce_backend(),
        SaveRamKind::none(),
        zeff_pce_core::hardware::WORK_RAM_LEN,
        zeff_pce_core::hardware::VDC_VRAM_BYTES,
    );
}

#[test]
fn backend_state_decode_smoke_covers_every_supported_core() {
    assert_backend_state_decode_smoke(build_gb_backend());
    assert_backend_state_decode_smoke(build_gba_backend());
    assert_backend_state_decode_smoke(build_nes_backend());
    assert_backend_state_decode_smoke(build_coleco_backend());
    assert_backend_state_decode_smoke(build_pce_backend());
    assert_backend_state_decode_smoke(build_ws_backend());
    assert_backend_state_decode_smoke(build_sms_backend());
}

fn assert_backend_state_decode_smoke(mut backend: EmuBackend) {
    let state = backend
        .encode_state_bytes()
        .expect("backend should encode state");
    backend.step_frame();
    backend
        .load_state_from_bytes(state)
        .expect("backend should decode its own state");
    backend.step_frame();
    assert!(!backend.framebuffer().is_empty());
}

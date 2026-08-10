use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, ROM_EXTENSIONS, load_backend_from_rom_source,
    system_specs,
};
use crate::debug::DebugUiActions;
use crate::emu_thread::EmuThread;
use std::collections::BTreeSet;
use std::path::PathBuf;
use zeff_emu_common::debug::WatchType;
use zeff_emu_common::save_ram::SaveRamKind;

mod fixtures;

use fixtures::*;

#[test]
fn active_system_detects_supported_rom_extensions() {
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("GAME.GB")),
        Some(ActiveSystem::GameBoy)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.gbc")),
        Some(ActiveSystem::GameBoy)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.sgb")),
        Some(ActiveSystem::GameBoy)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.gba")),
        Some(ActiveSystem::GameBoyAdvance)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.nes")),
        Some(ActiveSystem::Nes)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.ws")),
        Some(ActiveSystem::WonderSwan)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.wsc")),
        Some(ActiveSystem::WonderSwan)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.sms")),
        Some(ActiveSystem::MasterSystem)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.gg")),
        Some(ActiveSystem::GameGear)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.sg")),
        Some(ActiveSystem::Sg1000)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.sc")),
        Some(ActiveSystem::Sg1000)
    );
    assert_eq!(ActiveSystem::from_path(&PathBuf::from("game.7z")), None);
}

#[test]
fn system_specs_cover_supported_rom_extensions() {
    let from_specs = system_specs()
        .iter()
        .flat_map(|spec| spec.rom_extensions.iter().copied())
        .collect::<BTreeSet<_>>();
    let from_constant = ROM_EXTENSIONS.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(from_specs, from_constant);
    for spec in system_specs() {
        assert_eq!(
            ActiveSystem::from_extension(spec.short_code),
            Some(spec.system)
        );
        assert!(!spec.storage_subdir.is_empty());
        assert!(!spec.state_extension.is_empty());
        assert!(!spec.file_dialog_filter_name.is_empty());
    }
}

#[test]
fn shared_backend_loader_covers_every_supported_core() {
    let cases = [
        (
            ActiveSystem::GameBoy,
            "test.gb",
            build_gb_test_rom(),
            ActiveSystem::GameBoy,
        ),
        (
            ActiveSystem::GameBoyAdvance,
            "test.gba",
            build_gba_test_rom(),
            ActiveSystem::GameBoyAdvance,
        ),
        (
            ActiveSystem::Nes,
            "test.nes",
            build_nes_test_rom(),
            ActiveSystem::Nes,
        ),
        (
            ActiveSystem::WonderSwan,
            "test.ws",
            build_ws_test_rom(),
            ActiveSystem::WonderSwan,
        ),
        (
            ActiveSystem::MasterSystem,
            "test.sms",
            build_sms_test_rom(),
            ActiveSystem::MasterSystem,
        ),
    ];

    for (system, rom_name, rom, expected_backend_system) in cases {
        let backend = load_test_backend_with_shared_loader(system, rom_name, rom);
        assert_eq!(backend.system(), expected_backend_system);
        assert_eq!(backend.rom_path(), PathBuf::from(rom_name));
        assert_eq!(backend.source_path(), PathBuf::from(rom_name));
        assert!(!backend.framebuffer().is_empty());
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
}

#[test]
fn ws_backend_debug_actions_update_core_debug_state() {
    let rom = build_ws_test_rom();
    let mut emu = zeff_ws_core::emulator::Emulator::new(&rom, 44_100)
        .expect("WonderSwan emulator should initialize");
    let mut actions = DebugUiActions::none();
    actions.add_breakpoint = Some(0xF0000);
    actions.add_watchpoint = Some((0x0000, WatchType::Write));
    actions.memory_writes.push((0x0000, 0x5A));

    EmuThread::apply_ws_debug_actions(&mut emu, &actions);

    assert_eq!(emu.iter_breakpoints().collect::<Vec<_>>(), vec![0xF0000]);
    assert_eq!(emu.debug_watchpoints().len(), 1);
    assert_eq!(
        emu.debug_hit_watchpoint()
            .map(|hit| (hit.address, hit.new_value)),
        Some((0x0000, 0x5A))
    );
}

#[test]
fn gb_backend_smoke_roundtrip() {
    let mut backend = build_gb_backend();

    assert_eq!(backend.system(), ActiveSystem::GameBoy);
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::GameBoy.framebuffer_len()
    );
    assert!(backend.is_running());

    backend.step_frame();

    let state = backend
        .encode_state_bytes()
        .expect("GB backend should encode save-state");
    backend
        .load_state_from_bytes(state)
        .expect("GB backend should load save-state");
}

#[test]
fn nes_backend_smoke_roundtrip() {
    let mut backend = build_nes_backend();

    assert_eq!(backend.system(), ActiveSystem::Nes);
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::Nes.framebuffer_len()
    );
    assert!(backend.is_running());

    backend.step_frame();

    let state = backend
        .encode_state_bytes()
        .expect("NES backend should encode save-state");
    backend
        .load_state_from_bytes(state)
        .expect("NES backend should load save-state");
}

#[test]
fn gba_backend_smoke_roundtrip() {
    let mut backend = build_gba_backend();

    assert_eq!(backend.system(), ActiveSystem::GameBoyAdvance);
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::GameBoyAdvance.framebuffer_len()
    );
    assert!(backend.is_running());

    backend.step_frame();

    let state = backend
        .encode_state_bytes()
        .expect("GBA backend should encode save-state");
    backend
        .load_state_from_bytes(state)
        .expect("GBA backend should load save-state");
}

#[test]
fn ws_backend_smoke_roundtrip() {
    let mut backend = build_ws_backend();

    assert_eq!(backend.system(), ActiveSystem::WonderSwan);
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::WonderSwan.framebuffer_len()
    );
    assert!(backend.is_running());

    backend.step_frame();

    let state = backend
        .encode_state_bytes()
        .expect("WonderSwan backend should encode save-state");
    backend
        .load_state_from_bytes(state)
        .expect("WonderSwan backend should load save-state");
}

#[test]
fn sega8_backend_smoke_roundtrip() {
    let mut backend = build_sms_backend();

    assert_eq!(backend.system(), ActiveSystem::MasterSystem);
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::MasterSystem.framebuffer_len()
    );
    assert!(backend.is_running());

    backend.step_frame();

    let state = backend
        .encode_state_bytes()
        .expect("Sega 8-bit backend should encode save-state");
    backend
        .load_state_from_bytes(state)
        .expect("Sega 8-bit backend should load save-state");
}

#[test]
fn gb_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_gb_backend(), 1, 2);
}

#[test]
fn nes_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_nes_backend(), 1, 2);
}

#[test]
fn gba_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_gba_backend(), 1, 2);
}

#[test]
fn ws_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_ws_backend(), 1, 2);
}

#[test]
fn sega8_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_sms_backend(), 1, 2);
}

#[test]
fn gba_backend_tracks_logical_rom_path_and_reload_source_path_separately() {
    let rom = build_gba_test_rom();
    let gba = zeff_gba_core::emulator::Emulator::new(&rom, 44_100)
        .expect("GBA emulator should initialize");
    let backend = EmuBackend::from_gba_with_source(
        gba,
        PathBuf::from("inside_archive.gba"),
        PathBuf::from("archive.zip"),
    );

    assert_eq!(backend.rom_path(), PathBuf::from("inside_archive.gba"));
    assert_eq!(backend.source_path(), PathBuf::from("archive.zip"));
}

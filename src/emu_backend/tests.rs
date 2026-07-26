use super::{ActiveSystem, EmuBackend};
use std::path::PathBuf;

fn build_gb_test_rom() -> Vec<u8> {
    vec![0u8; 0x8000]
}

fn build_nes_test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;

    let prg = 16;
    rom[prg] = 0xA9;
    rom[prg + 1] = 0x42;
    rom[prg + 2] = 0x85;
    rom[prg + 3] = 0x00;
    rom[prg + 4] = 0xEA;
    rom[prg + 5] = 0xEA;

    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

fn build_gba_test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom
}

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
    assert_eq!(ActiveSystem::from_path(&PathBuf::from("game.7z")), None);
}

#[test]
fn gb_backend_smoke_roundtrip() {
    let rom = build_gb_test_rom();
    let gb = zeff_gb_core::emulator::Emulator::from_rom_data(
        &rom,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
    )
    .expect("GB emulator should initialize");

    let mut backend = EmuBackend::from_gb(gb, PathBuf::from("test.gb"));

    assert_eq!(backend.system(), ActiveSystem::GameBoy);
    assert_eq!(backend.framebuffer().len(), (160 * 144 * 4) as usize);
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
    let rom = build_nes_test_rom();
    let nes = zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0)
        .expect("NES emulator should initialize");

    let mut backend = EmuBackend::from_nes(nes, PathBuf::from("test.nes"));

    assert_eq!(backend.system(), ActiveSystem::Nes);
    assert_eq!(backend.framebuffer().len(), (256 * 240 * 4) as usize);
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
    let rom = build_gba_test_rom();
    let gba = zeff_gba_core::emulator::Emulator::new(&rom, 44_100)
        .expect("GBA emulator should initialize");

    let mut backend = EmuBackend::from_gba(gba, PathBuf::from("test.gba"));

    assert_eq!(backend.system(), ActiveSystem::GameBoyAdvance);
    assert_eq!(backend.framebuffer().len(), (240 * 160 * 4) as usize);
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

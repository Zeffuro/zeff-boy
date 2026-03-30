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

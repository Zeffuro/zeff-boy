use super::{ActiveSystem, EmuBackend, ROM_EXTENSIONS, system_specs};
use std::collections::BTreeSet;
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

fn build_ws_test_rom() -> Vec<u8> {
    let mut rom = vec![0xFF; 0x10000];
    rom[0] = 0xF4;
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer + 4] = 0x01;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn build_sms_test_rom() -> Vec<u8> {
    vec![0x76]
}

fn build_gb_backend() -> EmuBackend {
    let rom = build_gb_test_rom();
    let gb = zeff_gb_core::emulator::Emulator::from_rom_data(
        &rom,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
    )
    .expect("GB emulator should initialize");
    EmuBackend::from_gb(gb, PathBuf::from("test.gb"))
}

fn build_nes_backend() -> EmuBackend {
    let rom = build_nes_test_rom();
    let nes = zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0)
        .expect("NES emulator should initialize");
    EmuBackend::from_nes(nes, PathBuf::from("test.nes"))
}

fn build_gba_backend() -> EmuBackend {
    let rom = build_gba_test_rom();
    let gba = zeff_gba_core::emulator::Emulator::new(&rom, 44_100)
        .expect("GBA emulator should initialize");
    EmuBackend::from_gba(gba, PathBuf::from("test.gba"))
}

fn build_ws_backend() -> EmuBackend {
    let rom = build_ws_test_rom();
    let ws = zeff_ws_core::emulator::Emulator::new(&rom, 44_100)
        .expect("WonderSwan emulator should initialize");
    EmuBackend::from_ws(ws, PathBuf::from("test.ws"))
}

fn build_sms_backend() -> EmuBackend {
    let rom = build_sms_test_rom();
    let sms = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &rom,
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .expect("SMS emulator should initialize");
    EmuBackend::from_sega8(sms, PathBuf::from("test.sms"))
}

fn step_frames(backend: &mut EmuBackend, count: usize) {
    for _ in 0..count {
        backend.step_frame();
    }
}

fn assert_save_state_replay_is_deterministic(
    mut backend: EmuBackend,
    frames_before_checkpoint: usize,
    frames_after_checkpoint: usize,
) {
    step_frames(&mut backend, frames_before_checkpoint);

    let checkpoint_framebuffer = backend.framebuffer().to_vec();
    let checkpoint_state = backend
        .encode_state_bytes()
        .expect("backend should encode checkpoint save-state");

    step_frames(&mut backend, frames_after_checkpoint);

    let expected_framebuffer = backend.framebuffer().to_vec();
    let expected_state = backend
        .encode_state_bytes()
        .expect("backend should encode replay target save-state");

    backend
        .load_state_from_bytes(checkpoint_state)
        .expect("backend should restore checkpoint save-state");

    assert_eq!(
        backend.framebuffer(),
        checkpoint_framebuffer,
        "loading a save-state should restore the checkpoint framebuffer"
    );

    step_frames(&mut backend, frames_after_checkpoint);

    assert_eq!(
        backend.framebuffer(),
        expected_framebuffer,
        "replaying from a save-state should reproduce the same framebuffer"
    );
    assert_eq!(
        backend
            .encode_state_bytes()
            .expect("backend should encode replayed save-state"),
        expected_state,
        "replaying from a save-state should reproduce the same encoded state"
    );
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

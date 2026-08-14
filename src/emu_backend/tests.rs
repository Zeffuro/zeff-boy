use super::firmware::firmware_plan_for_active_system;
use super::{
    ActiveSystem, BackendLoadConfig, BackendRuntimeConfig, EmuBackend, ROM_EXTENSIONS,
    load_backend_from_rom_source, system_specs,
};
use crate::debug::DebugUiActions;
use crate::emu_core_trait::DebuggableEmulator;
use std::collections::BTreeSet;
use std::path::PathBuf;
use zeff_emu_common::debug::WatchType;
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_gb_core::hardware::types::constants::{INTERRUPT_IF, SERIAL_SB, SERIAL_SC};

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
        ActiveSystem::from_path(&PathBuf::from("game.fds")),
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
    assert!(!loaded.backend.framebuffer().is_empty());
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
            let loaded = load_backend_from_rom_source(
                spec.system,
                &rom_path,
                &rom_path,
                Some(rom),
                BackendLoadConfig {
                    sample_rate: Some(44_100),
                    ..BackendLoadConfig::default()
                },
            )
            .unwrap_or_else(|err| {
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

#[test]
fn backend_link_peer_sync_exchanges_game_boy_bytes() {
    let mut left = build_gb_backend();
    let mut right = build_gb_backend();

    assert!(left.sync_link_peer(&mut right));

    {
        let (EmuBackend::Gb(left), EmuBackend::Gb(right)) = (&mut left, &mut right) else {
            panic!("expected GB backends");
        };
        left.emu.write_byte(SERIAL_SB, 0xAB);
        right.emu.write_byte(SERIAL_SB, 0x34);
        left.emu.write_byte(SERIAL_SC, 0x81);
        right.emu.write_byte(SERIAL_SC, 0x80);
    }

    left.step_frame();
    right.step_frame();

    assert!(left.sync_link_peer(&mut right));

    let (EmuBackend::Gb(left), EmuBackend::Gb(right)) = (&left, &right) else {
        panic!("expected GB backends");
    };
    assert_eq!(left.emu.cpu_peek8(SERIAL_SB), 0x34);
    assert_eq!(right.emu.cpu_peek8(SERIAL_SB), 0xAB);
    assert_eq!(left.emu.cpu_peek8(SERIAL_SC) & 0x80, 0);
    assert_eq!(right.emu.cpu_peek8(SERIAL_SC) & 0x80, 0);
    assert_eq!(left.emu.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    assert_eq!(right.emu.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
}

#[test]
fn backend_link_peer_sync_exchanges_wonder_swan_uart_bytes() {
    let mut left = build_ws_backend();
    let mut right = build_ws_backend();

    assert!(left.sync_link_peer(&mut right));

    {
        let (EmuBackend::Ws(left), EmuBackend::Ws(right)) = (&mut left, &mut right) else {
            panic!("expected WonderSwan backends");
        };
        left.emu.io_write8(0x00B3, 0x80);
        right.emu.io_write8(0x00B3, 0x80);
        left.emu.io_write8(0x00B1, 0x5A);
    }

    for _ in 0..64 {
        left.step_frame();
        assert!(left.sync_link_peer(&mut right));
        let EmuBackend::Ws(right) = &right else {
            panic!("expected WonderSwan backend");
        };
        if right.emu.io_peek8(0x00B3) & 0x01 != 0 {
            break;
        }
    }

    let EmuBackend::Ws(right) = &right else {
        panic!("expected WonderSwan backend");
    };
    assert_eq!(right.emu.io_peek8(0x00B3) & 0x01, 0x01);
    assert_eq!(right.emu.io_peek8(0x00B1), 0x5A);
}

#[test]
fn backend_link_peer_sync_rejects_incompatible_pairs() {
    let mut gb = build_gb_backend();
    let mut gba = build_gba_backend();

    assert!(!gb.sync_link_peer(&mut gba));
}

fn test_rom_for_system(system: ActiveSystem) -> Vec<u8> {
    match system {
        ActiveSystem::GameBoy => build_gb_test_rom(),
        ActiveSystem::GameBoyAdvance => build_gba_test_rom(),
        ActiveSystem::Nes => build_nes_test_rom(),
        ActiveSystem::WonderSwan => build_ws_test_rom(),
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => {
            build_sms_test_rom()
        }
    }
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
    assert_backend_feature_contract(
        load_test_backend_with_shared_loader(ActiveSystem::Sg1000, "test.sg", build_sms_test_rom()),
        ActiveSystem::Sg1000,
        SaveRamKind::mapper_ram_unknown(
            zeff_sega8_core::hardware::constants::SMS_CARTRIDGE_RAM_SIZE,
        ),
        zeff_sega8_core::hardware::constants::SG_WORK_RAM_SIZE,
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
    assert_app_snapshot_core_features(
        load_test_backend_with_shared_loader(ActiveSystem::Sg1000, "test.sg", build_sms_test_rom()),
        SaveRamKind::mapper_ram_unknown(
            zeff_sega8_core::hardware::constants::SMS_CARTRIDGE_RAM_SIZE,
        ),
        zeff_sega8_core::hardware::constants::SG_WORK_RAM_SIZE,
        zeff_sega8_core::hardware::constants::SMS_VRAM_SIZE,
    );
}

#[test]
fn backend_state_decode_smoke_covers_every_supported_core() {
    assert_backend_state_decode_smoke(build_gb_backend());
    assert_backend_state_decode_smoke(build_gba_backend());
    assert_backend_state_decode_smoke(build_nes_backend());
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

#[test]
fn sega8_backend_exposes_semantic_audio_frame_for_recording() {
    let mut backend = build_sms_backend();
    backend.step_frame();

    let frame = backend
        .audio_semantic_frame()
        .expect("Sega 8-bit should expose PSG semantic audio data for recording");
    assert_eq!(frame.voices.len(), 4);
    assert_eq!(frame.voices[0].name, "Sega PSG Tone 0");
    assert_eq!(
        frame.voices[3].class,
        crate::audio_tooling::AudioVoiceClass::Noise
    );
    assert_eq!(frame.voices[3].pitch_hz, None);
}

#[test]
fn gba_backend_exposes_semantic_audio_frame_for_recording() {
    let mut backend = build_gba_backend();
    backend.step_frame();

    let frame = backend
        .audio_semantic_frame()
        .expect("GBA should expose PSG and FIFO semantic audio data for recording/tooling");
    assert_eq!(frame.voices.len(), 6);
    assert_eq!(frame.voices[0].name, "GBA PSG 1 (Square + Sweep)");
    assert_eq!(
        frame.voices[2].class,
        crate::audio_tooling::AudioVoiceClass::Wavetable
    );
    assert_eq!(
        frame.voices[3].class,
        crate::audio_tooling::AudioVoiceClass::Noise
    );
    assert_eq!(frame.voices[3].pitch_hz, None);
    assert_eq!(
        frame.voices[4].class,
        crate::audio_tooling::AudioVoiceClass::Pcm
    );
    assert_eq!(frame.voices[4].name, "GBA FIFO A");
    assert_eq!(frame.voices[4].pitch_hz, None);
}

#[test]
fn ws_backend_exposes_semantic_audio_frame_for_recording() {
    let mut backend = build_ws_backend();
    backend.step_frame();

    let frame = backend.audio_semantic_frame().expect(
        "WonderSwan should expose wave/noise/PCM semantic audio data for recording/tooling",
    );
    assert_eq!(frame.voices.len(), 5);
    assert_eq!(frame.voices[0].name, "WS CH0 Wave");
    assert_eq!(
        frame.voices[0].class,
        crate::audio_tooling::AudioVoiceClass::Wavetable
    );
    assert_eq!(
        frame.voices[4].class,
        crate::audio_tooling::AudioVoiceClass::Pcm
    );
    assert_eq!(frame.voices[4].name, "WS HyperVoice");
    assert_eq!(frame.voices[4].pitch_hz, None);
}

#[test]
fn debuggable_adapter_exposes_uniform_cpu_peek_write() {
    let mut gb = zeff_gb_core::emulator::Emulator::from_rom_data(
        &build_gb_test_rom(),
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
    )
    .expect("Game Boy emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut gb, 0xC000, 0x12);

    let mut gba = zeff_gba_core::emulator::Emulator::new(&build_gba_test_rom(), 44_100)
        .expect("GBA emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut gba, 0x0200_0000, 0x34);

    let mut nes = zeff_nes_core::emulator::Emulator::new(&build_nes_test_rom(), 44_100.0)
        .expect("NES emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut nes, 0x0000, 0x56);

    let mut ws = zeff_ws_core::emulator::Emulator::new(&build_ws_test_rom(), 44_100)
        .expect("WonderSwan emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut ws, 0x0000_1234, 0x78);

    let mut sega8 = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &build_sms_test_rom(),
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .expect("Sega 8-bit emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut sega8, 0xC123, 0x9A);
}

fn assert_debuggable_cpu_byte_access(
    emu: &mut impl DebuggableEmulator,
    address: zeff_emu_common::address::Address,
    value: u8,
) {
    emu.cpu_write8(address, value);
    assert_eq!(emu.cpu_peek8(address), value);
}

#[test]
fn ws_backend_debug_actions_update_core_debug_state() {
    let rom = build_ws_test_rom();
    let emu = zeff_ws_core::emulator::Emulator::new(&rom, 44_100)
        .expect("WonderSwan emulator should initialize");
    let mut backend = EmuBackend::from_ws(emu, PathBuf::from("test.ws"));
    let mut actions = DebugUiActions::none();
    actions.add_breakpoint = Some(0xF0000);
    actions.add_watchpoint = Some((0x0000, WatchType::Write));
    actions.memory_writes.push((0x0000, 0x5A));

    backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));

    let ws = backend
        .ws()
        .expect("backend should remain WonderSwan after debug actions");
    assert_eq!(ws.emu.iter_breakpoints().collect::<Vec<_>>(), vec![0xF0000]);
    assert_eq!(ws.emu.debug_watchpoints().len(), 1);
    assert_eq!(
        ws.emu
            .debug_hit_watchpoint()
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
fn gb_rtc_replay_hash_ignores_bess_wall_clock_timestamp() {
    let backend = load_test_backend_with_shared_loader(
        ActiveSystem::GameBoy,
        "rtc.gbc",
        build_gb_mbc3_rtc_test_rom(),
    );

    let mut canonicalized_raw = backend
        .encode_state_bytes()
        .expect("GB RTC backend should encode save-state");
    let replay_hash_state = backend
        .encode_replay_hash_state_bytes()
        .expect("GB RTC backend should encode replay-hash state");

    assert_ne!(
        replay_hash_state, canonicalized_raw,
        "raw GB RTC save-state should include a BESS wall-clock timestamp"
    );

    zeff_gb_core::save_state::canonicalize_replay_hash_bytes(&mut canonicalized_raw);

    assert_eq!(
        replay_hash_state, canonicalized_raw,
        "replay-hash state should only canonicalize timestamp noise"
    );
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
fn gb_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_gb_backend(),
        build_gb_backend(),
    );
}

#[test]
fn nes_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_nes_backend(),
        build_nes_backend(),
    );
}

#[test]
fn gba_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_gba_backend(),
        build_gba_backend(),
    );
}

#[test]
fn ws_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_ws_backend(),
        build_ws_backend(),
    );
}

#[test]
fn sega8_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_sms_backend(),
        build_sms_backend(),
    );
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

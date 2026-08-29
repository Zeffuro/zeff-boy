use super::firmware::{
    default_firmware_manifests_for_active_system, firmware_plan_for_active_system,
};
use super::{
    ActiveSystem, BackendLoadConfig, BackendRuntimeConfig, EmuBackend, ROM_EXTENSIONS,
    load_backend_from_rom_source, system_specs,
};
use crate::cheats::{CheatPatch, CheatValue};
use crate::debug::DebugUiActions;
use crate::emu_core_trait::DebuggableEmulator;
use crate::emu_thread::GuestCallRequest;
use crate::symbols::ExecMode;
use std::collections::BTreeSet;
use std::path::PathBuf;
use zeff_emu_common::debug::{DebugEvent, TraceExecMode, WatchType};
use zeff_emu_common::memory::MemoryRegionDescriptor;
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset};
use zeff_gb_core::hardware::types::constants::{INTERRUPT_IF, SERIAL_SB, SERIAL_SC};

mod fixtures;
mod runahead_conformance;

use fixtures::*;

static TEST_COLECO_BIOS: [u8; 8 * 1024] = [0; 8 * 1024];

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
        ActiveSystem::from_path(&PathBuf::from("game.col")),
        Some(ActiveSystem::Coleco)
    );
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.pce")),
        Some(ActiveSystem::Pce)
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
    assert_eq!(
        ActiveSystem::from_path(&PathBuf::from("game.iso")),
        Some(ActiveSystem::Pce)
    );
    assert_eq!(ActiveSystem::from_path(&PathBuf::from("game.rar")), None);
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
        for extension in spec.rom_extensions {
            assert_eq!(ActiveSystem::from_extension(extension), Some(spec.system));
        }
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
            ActiveSystem::Pce,
            "test.pce",
            build_pce_test_rom(),
            ActiveSystem::Pce,
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

        assert_frame_lifecycle_roundtrip(backend);
    }
}

#[test]
fn detached_speculation_forks_raw_supported_cores_without_mutating_primary_backends() {
    let sega8 = build_sms_backend();
    let state_before = sega8.encode_state_bytes().unwrap();
    let framebuffer_before = sega8.framebuffer().to_vec();
    let battery_before = sega8.battery_component_hash();

    let mut detached = sega8
        .fork_detached_for_speculation()
        .expect("Sega8 should support an in-memory detached fork");
    detached.disable_audio_output();
    let detached_frame_before = detached.frame_count();
    assert!(detached.step_frames(1));
    assert_eq!(detached.frame_count(), detached_frame_before + 1);

    assert_eq!(sega8.encode_state_bytes().unwrap(), state_before);
    assert_eq!(sega8.framebuffer(), framebuffer_before);
    assert_eq!(sega8.battery_component_hash(), battery_before);

    let rom = build_gba_sram_rtc_test_rom();
    let mut gba_emu = zeff_gba_core::emulator::Emulator::new(&rom, 44_100).unwrap();
    let sram = (0..gba_emu.dump_battery_sram().unwrap().len())
        .map(|index| (index as u8).wrapping_mul(17).wrapping_add(3))
        .collect::<Vec<_>>();
    gba_emu.load_battery_sram(&sram).unwrap();
    let rtc =
        zeff_gba_core::hardware::cartridge::RtcDateTime::new(2040, 2, 29, 4, [23, 59, 59]).unwrap();
    assert!(gba_emu.set_rtc_date_time(rtc));
    let gba_state_before_partial_gpio = gba_emu.encode_state().unwrap();
    gba_begin_partial_rtc_gpio_command(&mut gba_emu);
    assert!(gba_emu.encode_state().unwrap() != gba_state_before_partial_gpio);
    let gba = EmuBackend::from_gba(gba_emu, PathBuf::from("emerald-sram.gba"));
    let gba_state_before = gba.encode_state_bytes().unwrap();
    let gba_framebuffer_before = gba.framebuffer().to_vec();
    let gba_battery_before = match &gba {
        EmuBackend::Gba(backend) => backend.emu.dump_battery_sram().unwrap(),
        _ => unreachable!(),
    };
    let gba_rtc_before = match &gba {
        EmuBackend::Gba(backend) => backend.emu.rtc_date_time(),
        _ => unreachable!(),
    };

    let mut detached = gba
        .fork_detached_for_speculation()
        .expect("GBA should support an in-memory detached fork");
    detached.disable_audio_output();
    match &detached {
        super::DetachedFrameBackend::Gba { emu, .. } => {
            assert_eq!(emu.rtc_date_time(), Some(rtc));
            assert_eq!(emu.dump_battery_sram().unwrap(), sram);
            assert!(emu.encode_state().unwrap() == gba_state_before);
        }
        _ => panic!("expected detached GBA backend"),
    }
    let detached_frame_before = detached.frame_count();
    assert!(detached.step_frames(1));
    assert_eq!(detached.frame_count(), detached_frame_before + 1);
    match &mut detached {
        super::DetachedFrameBackend::Gba { emu, .. } => {
            let state_before_gpio_completion = emu.encode_state().unwrap();
            assert_eq!(
                gba_complete_partial_rtc_gpio_command(emu),
                [0x40, 0x02, 0x29, 0x04, 0x23, 0x59, 0x59]
            );
            assert!(emu.encode_state().unwrap() != state_before_gpio_completion);
            assert_eq!(emu.rtc_date_time(), Some(rtc));
            assert_eq!(emu.dump_battery_sram().unwrap(), sram);
        }
        _ => panic!("expected detached GBA backend"),
    }
    let detached_rtc =
        zeff_gba_core::hardware::cartridge::RtcDateTime::new(2044, 2, 29, 1, [3, 5, 7]).unwrap();
    match &mut detached {
        super::DetachedFrameBackend::Gba { emu, .. } => {
            let detached_sram = vec![0xA7; sram.len()];
            emu.load_battery_sram(&detached_sram).unwrap();
            assert!(emu.set_rtc_date_time(detached_rtc));
            assert_eq!(emu.dump_battery_sram().unwrap(), detached_sram);
            assert_eq!(emu.rtc_date_time(), Some(detached_rtc));
        }
        _ => panic!("expected detached GBA backend"),
    }

    assert_eq!(gba.encode_state_bytes().unwrap(), gba_state_before);
    assert_eq!(gba.framebuffer(), gba_framebuffer_before);
    assert_eq!(
        match &gba {
            EmuBackend::Gba(backend) => backend.emu.dump_battery_sram().unwrap(),
            _ => unreachable!(),
        },
        gba_battery_before
    );
    assert_eq!(gba_battery_before, sram);
    assert_eq!(
        match &gba {
            EmuBackend::Gba(backend) => backend.emu.rtc_date_time(),
            _ => unreachable!(),
        },
        gba_rtc_before
    );
    assert_eq!(gba_rtc_before, Some(rtc));

    assert!(
        build_pce_backend()
            .fork_detached_for_speculation()
            .is_none()
    );

    for hint in [
        zeff_sega8_core::hardware::cartridge::SystemHint::GameGear,
        zeff_sega8_core::hardware::cartridge::SystemHint::Sg1000,
    ] {
        let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(&[0x00], 44_100, hint)
            .expect("non-SMS Sega8 fixture should initialize");
        let backend = EmuBackend::from_sega8(emu, PathBuf::from("unsupported.sega8"));
        assert!(!backend.supports_detached_speculation());
        assert!(backend.fork_detached_for_speculation().is_none());
    }
}

fn gba_begin_partial_rtc_gpio_command(emu: &mut zeff_gba_core::emulator::Emulator) {
    emu.cpu_write16(0x0800_00C8, 1);
    emu.cpu_write16(0x0800_00C4, 1);
    emu.cpu_write16(0x0800_00C4, 5);
    emu.cpu_write16(0x0800_00C6, 7);
    for bit in (5..=7).rev() {
        gba_write_rtc_gpio_bit(emu, (0x65 >> bit) & 1);
    }
}

fn gba_complete_partial_rtc_gpio_command(emu: &mut zeff_gba_core::emulator::Emulator) -> [u8; 7] {
    for bit in (0..=4).rev() {
        gba_write_rtc_gpio_bit(emu, (0x65 >> bit) & 1);
    }
    emu.cpu_write16(0x0800_00C6, 5);
    let mut bytes = [0; 7];
    for byte in &mut bytes {
        for bit in 0..8 {
            emu.cpu_write16(0x0800_00C4, 4);
            emu.cpu_write16(0x0800_00C4, 5);
            *byte |= ((emu.cpu_peek16(0x0800_00C4) as u8 >> 1) & 1) << bit;
        }
    }
    emu.cpu_write16(0x0800_00C4, 4);
    bytes
}

fn gba_write_rtc_gpio_bit(emu: &mut zeff_gba_core::emulator::Emulator, bit: u8) {
    emu.cpu_write16(0x0800_00C4, u16::from(4 | ((bit & 1) << 1)));
    emu.cpu_write16(0x0800_00C4, u16::from(5 | ((bit & 1) << 1)));
}

#[test]
fn gba_detached_rtc_route_is_exact_across_emerald_regions_and_non_rtc_control() {
    let seed =
        zeff_gba_core::hardware::cartridge::RtcDateTime::new(2040, 2, 29, 4, [23, 59, 59]).unwrap();
    for (game_code, expected_rtc) in [(*b"BPEE", true), (*b"BPEJ", true), (*b"AXVE", false)] {
        let rom = build_gba_sram_test_rom(game_code);
        let mut emu = zeff_gba_core::emulator::Emulator::new(&rom, 44_100).unwrap();
        assert_eq!(
            emu.cartridge_header().game_code.as_bytes(),
            game_code.as_slice()
        );
        assert_eq!(emu.has_rtc(), expected_rtc);
        assert_eq!(emu.set_rtc_date_time(seed), expected_rtc);
        assert_eq!(emu.rtc_date_time(), expected_rtc.then_some(seed));

        let backend = EmuBackend::from_gba(emu, PathBuf::from("rtc-route.gba"));
        assert!(backend.supports_detached_speculation());
        let detached = backend
            .fork_detached_for_speculation()
            .expect("GBA raw core should fork");
        match detached {
            super::DetachedFrameBackend::Gba { emu, .. } => {
                assert_eq!(emu.has_rtc(), expected_rtc);
                assert_eq!(emu.rtc_date_time(), expected_rtc.then_some(seed));
            }
            _ => panic!("expected detached GBA backend"),
        }
    }
}

#[test]
fn nes_backend_pacing_follows_header_declared_pal_and_dendy_timing() {
    for (header_byte_7, header_byte_9, header_byte_12, expected_rate) in [
        (0x00, 0x01, 0x00, (53_203_425, 32)),
        (0x08, 0x00, 0x03, (53_203_425, 30)),
    ] {
        let mut rom = build_nes_test_rom();
        rom[7] = header_byte_7;
        rom[9] = header_byte_9;
        rom[12] = header_byte_12;
        let nes = zeff_nes_core::emulator::Emulator::new(&rom, 44_100.0)
            .expect("regional NES emulator should initialize");
        let backend = EmuBackend::from_nes(nes, PathBuf::from("regional.nes"));

        assert_eq!(backend.nominal_frame_duration_ns(), 19_997_209);
        assert_eq!(
            backend.timing_snapshot().rate(),
            zeff_emu_common::time::ClockRate::from_ratio(expected_rate.0, expected_rate.1)
        );
    }
}

#[test]
fn nes_backend_soft_reset_preserves_public_frame_count() {
    let mut backend = build_nes_backend();
    FrameLifecycle::step_frame(&mut backend);
    let frame_before_reset = FrameLifecycle::frame_count(&backend);

    Reset::reset(&mut backend);

    assert_eq!(FrameLifecycle::frame_count(&backend), frame_before_reset);
}

fn assert_frame_lifecycle_roundtrip(mut backend: EmuBackend) {
    let before_frame = FrameLifecycle::frame_count(&backend);
    let before_timing = backend.timing_snapshot();

    FrameLifecycle::step_frame(&mut backend);

    let after_frame = FrameLifecycle::frame_count(&backend);
    let after_timing = backend.timing_snapshot();
    assert_eq!(after_frame, before_frame + 1);
    assert_eq!(before_timing.rate(), after_timing.rate());
    assert!(
        after_timing
            .elapsed_since(before_timing)
            .is_some_and(|ticks| ticks.get() > 0)
    );

    if !backend.supports_state_capture() {
        Reset::reset(&mut backend);
        assert_eq!(FrameLifecycle::frame_count(&backend), before_frame);
        assert_eq!(backend.timing_snapshot(), before_timing);
        return;
    }

    let state = backend.encode_state_bytes().unwrap();
    FrameLifecycle::step_frame(&mut backend);
    assert!(
        backend
            .timing_snapshot()
            .elapsed_since(after_timing)
            .is_some_and(|ticks| ticks.get() > 0)
    );
    backend.load_state_from_bytes(state).unwrap();
    assert_eq!(backend.timing_snapshot(), after_timing);

    let expected_reset_frame = if backend.system() == ActiveSystem::Nes {
        FrameLifecycle::frame_count(&backend)
    } else {
        before_frame
    };
    Reset::reset(&mut backend);
    assert_eq!(FrameLifecycle::frame_count(&backend), expected_reset_frame);
    assert_eq!(backend.timing_snapshot(), before_timing);
}

#[test]
fn failed_guest_call_restores_the_full_state() {
    let mut backend = build_gb_backend();
    backend.debug_suspend();
    let before = backend.encode_state_bytes().unwrap();
    let error = backend
        .execute_guest_call(&GuestCallRequest {
            name: "NeverReturns".to_owned(),
            target: 0x0150,
            storage_offset: None,
            explicit_overlay: false,
            exec_mode: ExecMode::Sm83,
            instruction_budget: 3,
        })
        .unwrap_err();

    assert!(error.to_string().contains("state restored"));
    assert_eq!(backend.encode_state_bytes().unwrap(), before);
}

#[test]
fn guest_call_rejects_a_stale_rom_mapping() {
    let mut backend = build_gb_backend();
    backend.debug_suspend();
    let error = backend
        .execute_guest_call(&GuestCallRequest {
            name: "WrongBank".to_owned(),
            target: 0x0150,
            storage_offset: Some(0x4150),
            explicit_overlay: false,
            exec_mode: ExecMode::Sm83,
            instruction_budget: 3,
        })
        .unwrap_err();

    assert!(error.to_string().contains("no longer maps"));
}

#[test]
fn pce_guest_call_returns_to_suspended_context_and_produces_undo_state() {
    let mut rom = build_pce_test_rom();
    rom[6..13].copy_from_slice(&[0xBA, 0xE8, 0xE8, 0x9A, 0x4C, 0x00, 0xE0]);
    let mut backend = load_test_backend_with_shared_loader(ActiveSystem::Pce, "call.pce", rom);
    backend.debug_suspend();

    let (instructions, undo_state) = backend
        .execute_guest_call(&GuestCallRequest {
            name: "GuestRoutine".to_owned(),
            target: 0xE006,
            storage_offset: Some(6),
            explicit_overlay: false,
            exec_mode: ExecMode::HuC6280,
            instruction_budget: 10,
        })
        .unwrap();

    assert_eq!(instructions, 5);
    assert!(!undo_state.is_empty());
    assert!(backend.is_suspended());
    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert_eq!(pce.debug_cpu_snapshot().registers().pc, 0xE000);
}

#[test]
fn coleco_guest_call_returns_to_suspended_context_and_produces_undo_state() {
    let mut rom = vec![0; 8 * 1024];
    rom[..2].copy_from_slice(&[0xAA, 0x55]);
    let mut bios = vec![0; zeff_coleco_core::constants::BIOS_SIZE];
    bios[..4].copy_from_slice(&[0x31, 0x00, 0x70, 0x00]);
    bios[0x100..0x103].copy_from_slice(&[0x3E, 0x42, 0xC9]);
    let mut emu = zeff_coleco_core::Emulator::new(&rom, &bios, 44_100).unwrap();
    emu.step_instruction();
    let mut backend = EmuBackend::from_coleco(
        emu,
        PathBuf::from("call.col"),
        crate::emu_backend::ColecoBackend::rom_hash_for_bytes(&rom),
    );
    backend.debug_suspend();

    let (instructions, undo_state) = backend
        .execute_guest_call(&GuestCallRequest {
            name: "GuestRoutine".to_owned(),
            target: 0x0100,
            storage_offset: None,
            explicit_overlay: false,
            exec_mode: ExecMode::Z80,
            instruction_budget: 10,
        })
        .unwrap();

    assert_eq!(instructions, 2);
    assert!(!undo_state.is_empty());
    assert!(backend.is_suspended());
    let coleco = backend.coleco().unwrap();
    assert_eq!(coleco.emu.cpu().regs().pc, 3);
    assert_eq!(coleco.emu.cpu().regs().a, 0x42);
}

#[test]
fn failed_pce_guest_call_restores_the_full_state() {
    let mut backend = build_pce_backend();
    backend.debug_suspend();
    let before = backend.encode_state_bytes().unwrap();
    let error = backend
        .execute_guest_call(&GuestCallRequest {
            name: "NeverReturns".to_owned(),
            target: 0xE001,
            storage_offset: Some(1),
            explicit_overlay: false,
            exec_mode: ExecMode::HuC6280,
            instruction_budget: 3,
        })
        .unwrap_err();

    assert!(error.to_string().contains("state restored"));
    assert_eq!(backend.encode_state_bytes().unwrap(), before);
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
    assert!(matches!(
        loaded.backend.replay_metadata().firmware.as_slice(),
        [zeff_emu_common::replay::ReplayFirmwareManifest::External {
            firmware_id,
            variant: Some(variant),
            sha256,
        }] if firmware_id == "nintendo.fds.bios"
            && variant == "test-override"
            && *sha256 == zeff_firmware::sha256_bytes(&TEST_FDS_BIOS)
    ));
    assert!(!loaded.backend.framebuffer().is_empty());
}

#[test]
fn shared_backend_loader_restores_fds_persistent_media_container() {
    static TEST_FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
        [0xFF; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];
    let mut fds_image = vec![0x55; zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE];
    fds_image[0] = 0x01;
    let seed = zeff_nes_core::emulator::Emulator::new_fds(
        &fds_image,
        TEST_FDS_BIOS.to_vec(),
        zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE,
    )
    .unwrap();
    let mut persistent = seed.dump_persistent_data().unwrap();
    *persistent.last_mut().unwrap() = 0xA7;

    let temp_dir = std::env::temp_dir().join(format!(
        "zeff_boy_fds_persistence_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let rom_path = temp_dir.join("media.fds");
    std::fs::write(rom_path.with_extension("sav"), &persistent).unwrap();

    let loaded = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &rom_path,
        &rom_path,
        Some(fds_image),
        BackendLoadConfig {
            fds_bios_override: Some(&TEST_FDS_BIOS),
            ..BackendLoadConfig::default()
        },
    )
    .expect("FDS loader should restore its persistent media container");

    let EmuBackend::Nes(backend) = loaded.backend else {
        panic!("FDS content should use the NES backend");
    };
    assert_eq!(backend.emu.dump_persistent_data().unwrap(), persistent);
    let _ = std::fs::remove_dir_all(temp_dir);
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
            let result = load_backend_from_rom_source(
                spec.system,
                &rom_path,
                &rom_path,
                Some(rom),
                BackendLoadConfig {
                    sample_rate: Some(44_100),
                    coleco_bios_override: (spec.system == ActiveSystem::Coleco)
                        .then_some(&TEST_COLECO_BIOS),
                    ..BackendLoadConfig::default()
                },
            );
            if matches!(*extension, "cue" | "chd" | "iso") {
                let error = match result {
                    Ok(_) => panic!("packaged PC Engine CD media unexpectedly loaded"),
                    Err(error) => error,
                };
                assert!(error.to_string().contains("PackagedCdSetUnsupported"));
                continue;
            }
            let loaded = result.unwrap_or_else(|err| {
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
    assert!(firmware_plan_for_active_system(ActiveSystem::Sg1000).is_empty());
    assert!(firmware_plan_for_active_system(ActiveSystem::Pce).is_empty());

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
fn shared_backend_loader_records_default_firmware_manifests() {
    for system in [
        ActiveSystem::GameBoy,
        ActiveSystem::GameBoyAdvance,
        ActiveSystem::Nes,
        ActiveSystem::Pce,
        ActiveSystem::WonderSwan,
        ActiveSystem::MasterSystem,
        ActiveSystem::GameGear,
        ActiveSystem::Sg1000,
    ] {
        let path = PathBuf::from(match system {
            ActiveSystem::GameBoy => "firmware.gb",
            ActiveSystem::GameBoyAdvance => "firmware.gba",
            ActiveSystem::Nes => "firmware.nes",
            ActiveSystem::Coleco => "firmware.col",
            ActiveSystem::Pce => "firmware.pce",
            ActiveSystem::WonderSwan => "firmware.ws",
            ActiveSystem::MasterSystem => "firmware.sms",
            ActiveSystem::GameGear => "firmware.gg",
            ActiveSystem::Sg1000 => "firmware.sg",
        });
        let loaded = load_backend_from_rom_source(
            system,
            &path,
            &path,
            Some(test_rom_for_system(system)),
            BackendLoadConfig::default(),
        )
        .unwrap_or_else(|err| panic!("{system:?} firmware metadata load failed: {err}"));

        assert_eq!(
            loaded.backend.replay_metadata().firmware,
            default_firmware_manifests_for_active_system(system)
        );
    }

    assert!(matches!(
        default_firmware_manifests_for_active_system(ActiveSystem::GameBoyAdvance).as_slice(),
        [zeff_emu_common::replay::ReplayFirmwareManifest::Hle {
            firmware_id,
            implementation,
            compatibility_version: 1,
        }] if firmware_id == "nintendo.gba.bios" && implementation == "zeff-gba-hle"
    ));
}

#[test]
#[ignore = "requires ZEFF_FIRMWARE_TEST_DIR with a retail GBA BIOS"]
fn shared_gba_loader_uses_selected_external_bios() {
    let root = PathBuf::from(std::env::var("ZEFF_FIRMWARE_TEST_DIR").unwrap());
    let path = PathBuf::from("firmware-test.gba");
    let loaded = load_backend_from_rom_source(
        ActiveSystem::GameBoyAdvance,
        &path,
        &path,
        Some(build_gba_test_rom()),
        BackendLoadConfig {
            firmware_search_dirs: vec![root],
            gba_use_external_bios: true,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap();

    assert_eq!(loaded.backend.gba().unwrap().emu.cpu_pc(), 0);
    assert!(matches!(
        loaded.backend.replay_metadata().firmware.as_slice(),
        [zeff_emu_common::replay::ReplayFirmwareManifest::External {
            firmware_id,
            ..
        }] if firmware_id == "nintendo.gba.bios"
    ));
}

#[test]
#[ignore = "requires ZEFF_FIRMWARE_TEST_DIR with a recognized ColecoVision BIOS"]
fn shared_coleco_loader_uses_recognized_external_bios() {
    let root = PathBuf::from(std::env::var("ZEFF_FIRMWARE_TEST_DIR").unwrap());
    let path = PathBuf::from("firmware-test.col");
    let loaded = load_backend_from_rom_source(
        ActiveSystem::Coleco,
        &path,
        &path,
        Some(test_rom_for_system(ActiveSystem::Coleco)),
        BackendLoadConfig {
            firmware_search_dirs: vec![root],
            ..BackendLoadConfig::default()
        },
    )
    .unwrap();

    let EmuBackend::Coleco(backend) = &loaded.backend else {
        panic!("expected ColecoVision backend");
    };
    assert_eq!(backend.emu.cpu().regs().pc, 0);
    assert!(matches!(
        loaded.backend.replay_metadata().firmware.as_slice(),
        [zeff_emu_common::replay::ReplayFirmwareManifest::External { firmware_id, .. }]
            if firmware_id == "coleco.vision.bios"
    ));
}

#[test]
#[ignore = "requires ZEFF_FIRMWARE_TEST_DIR with retail GB boot ROMs"]
fn shared_gb_loader_uses_boot_rom_for_selected_hardware() {
    let root = PathBuf::from(std::env::var("ZEFF_FIRMWARE_TEST_DIR").unwrap());
    let path = PathBuf::from("firmware-test.gbc");
    let mut rom = test_rom_for_system(ActiveSystem::GameBoy);
    rom[0x143] = 0x80;
    let loaded = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &path,
        &path,
        Some(rom),
        BackendLoadConfig {
            firmware_search_dirs: vec![root],
            gb_use_external_boot_rom: true,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap();

    let emu = &loaded.backend.gb().unwrap().emu;
    assert_eq!(emu.cpu_pc(), 0);
    assert!(emu.boot_rom_enabled());
    assert!(
        loaded
            .backend
            .replay_metadata()
            .firmware
            .iter()
            .any(|firmware| {
                matches!(
                    firmware,
                    zeff_emu_common::replay::ReplayFirmwareManifest::External { firmware_id, .. }
                        if firmware_id == "nintendo.gb.boot.cgb"
                )
            })
    );
}

#[test]
#[ignore = "requires ZEFF_FIRMWARE_TEST_DIR with retail Sega boot ROMs"]
fn shared_sega8_loader_uses_selected_boot_rom() {
    let root = PathBuf::from(std::env::var("ZEFF_FIRMWARE_TEST_DIR").unwrap());
    for (system, name, firmware_id) in [
        (
            ActiveSystem::MasterSystem,
            "firmware-test.sms",
            "sega.sms.boot",
        ),
        (ActiveSystem::GameGear, "firmware-test.gg", "sega.gg.boot"),
    ] {
        let path = PathBuf::from(name);
        let loaded = load_backend_from_rom_source(
            system,
            &path,
            &path,
            Some(test_rom_for_system(system)),
            BackendLoadConfig {
                firmware_search_dirs: vec![root.clone()],
                sega8_use_external_boot_rom: true,
                ..BackendLoadConfig::default()
            },
        )
        .unwrap();

        assert!(loaded.backend.sega8().unwrap().emu.bus().boot_rom_enabled());
        assert!(matches!(
            loaded.backend.replay_metadata().firmware.as_slice(),
            [zeff_emu_common::replay::ReplayFirmwareManifest::External {
                firmware_id: actual,
                ..
            }] if actual == firmware_id
        ));
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

    let (EmuBackend::Gb(left_gb), EmuBackend::Gb(right_gb)) = (&left, &right) else {
        panic!("expected GB backends");
    };
    assert_eq!(left_gb.emu.cpu_peek8(SERIAL_SB), 0x34);
    assert_eq!(right_gb.emu.cpu_peek8(SERIAL_SB), 0x34);
    assert_ne!(right_gb.emu.cpu_peek8(SERIAL_SC) & 0x80, 0);

    right.step_frame();

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

#[test]
fn sega8_link_sync_and_detached_factory_matrix_is_explicit() {
    let backend = |hint, name| {
        let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(&[0x00], 44_100, hint)
            .expect("Sega8 matrix fixture should initialize");
        EmuBackend::from_sega8(emu, PathBuf::from(name))
    };
    let mut sms_left = backend(
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        "left.sms",
    );
    let mut sms_right = backend(
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        "right.sms",
    );
    assert!(!sms_left.sync_link_peer(&mut sms_right));
    for sms in [&sms_left, &sms_right] {
        assert!(sms.supports_detached_speculation());
        assert!(sms.fork_detached_for_speculation().is_some());
    }

    let mut gg_left = backend(
        zeff_sega8_core::hardware::cartridge::SystemHint::GameGear,
        "left.gg",
    );
    let mut gg_right = backend(
        zeff_sega8_core::hardware::cartridge::SystemHint::GameGear,
        "right.gg",
    );
    assert!(gg_left.sync_link_peer(&mut gg_right));
    for game_gear in [&gg_left, &gg_right] {
        assert!(!game_gear.supports_detached_speculation());
        assert!(game_gear.fork_detached_for_speculation().is_none());
    }

    let mut sg1000 = backend(
        zeff_sega8_core::hardware::cartridge::SystemHint::Sg1000,
        "peer.sg",
    );
    assert!(!gg_left.sync_link_peer(&mut sg1000));
    assert!(!sg1000.supports_detached_speculation());
    assert!(sg1000.fork_detached_for_speculation().is_none());
}

fn test_rom_for_system(system: ActiveSystem) -> Vec<u8> {
    match system {
        ActiveSystem::GameBoy => build_gb_test_rom(),
        ActiveSystem::GameBoyAdvance => build_gba_test_rom(),
        ActiveSystem::Nes => build_nes_test_rom(),
        ActiveSystem::Coleco => {
            let mut rom = vec![0; 8 * 1024];
            rom[..2].copy_from_slice(&[0xAA, 0x55]);
            rom
        }
        ActiveSystem::Pce => build_pce_test_rom(),
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
fn pce_backend_exposes_bounded_frontend_and_debugger_capabilities() {
    let backend = build_pce_backend();
    let features = backend.capabilities();

    assert_eq!(backend.system(), ActiveSystem::Pce);
    assert_eq!(
        backend.core_family(),
        zeff_emu_common::system::CoreFamily::PcEngine
    );
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::Pce.framebuffer_len()
    );
    assert_eq!(
        features.system_ram_len,
        zeff_pce_core::hardware::WORK_RAM_LEN
    );
    assert_eq!(
        features.video_ram_len,
        zeff_pce_core::hardware::VDC_VRAM_BYTES
    );
    assert_eq!(
        features.memory_regions,
        vec![
            MemoryRegionDescriptor::cpu_address_space(16),
            MemoryRegionDescriptor::system_ram(zeff_pce_core::hardware::WORK_RAM_LEN),
            MemoryRegionDescriptor::video_ram(zeff_pce_core::hardware::VDC_VRAM_BYTES),
            MemoryRegionDescriptor::palette_ram(zeff_pce_core::hardware::VCE_PALETTE_COLORS * 2,),
            MemoryRegionDescriptor::oam(zeff_pce_core::hardware::VDC_SATB_WORDS * 2),
            MemoryRegionDescriptor::framebuffer(ActiveSystem::Pce.framebuffer_len()),
        ]
    );
    assert_eq!(
        features.input_features,
        crate::emu_backend::InputCapabilities::for_system(ActiveSystem::Pce)
    );
    assert_eq!(features.input_features.max_players, 5);
    assert!(features.supports_save_states);
    assert!(features.supports_state_capture);
    assert!(features.supports_rewind);
    assert!(features.supports_replay);
    assert!(features.supports_audio);
    assert!(features.supports_cheats);
    assert!(features.supports_guest_calls);
    assert!(features.supports_debugger);
    assert!(features.supports_execution_controls);
    assert!(features.supports_opcode_history);
    assert!(features.cheat_features.supports_user_cheats);
    assert!(features.cheat_features.supports_ram_writes);
    assert!(!features.cheat_features.supports_rom_patches);
    assert!(matches!(backend, EmuBackend::Pce(_)));
}

#[test]
fn pce_loader_preserves_direct_and_archive_paths() {
    let rom = build_pce_test_rom();
    let direct = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &PathBuf::from("direct.pce"),
        &PathBuf::from("direct.pce"),
        Some(rom.clone()),
        BackendLoadConfig::default(),
    )
    .unwrap();
    assert_eq!(direct.backend.rom_path(), PathBuf::from("direct.pce"));
    assert_eq!(direct.backend.source_path(), PathBuf::from("direct.pce"));

    let archive = PathBuf::from("collection.zip");
    let virtual_path = archive.join("folder/game.pce");
    let archived = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &archive,
        &virtual_path,
        Some(rom),
        BackendLoadConfig::default(),
    )
    .unwrap();
    assert_eq!(archived.backend.rom_path(), virtual_path);
    assert_eq!(archived.backend.source_path(), archive);
}

#[test]
fn pce_loader_classifies_structural_sf2_identically_for_direct_and_archive_sources() {
    let mut rom = build_pce_test_rom();
    rom.resize(zeff_pce_core::hardware::SF2_CE_HUCARD_IMAGE_LEN, 0xEA);
    for (source_path, rom_path) in [
        (PathBuf::from("sf2.pce"), PathBuf::from("sf2.pce")),
        (
            PathBuf::from("sf2.zip"),
            PathBuf::from("sf2.zip").join("Street Fighter II.pce"),
        ),
    ] {
        let loaded = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &source_path,
            &rom_path,
            Some(rom.clone()),
            BackendLoadConfig::default(),
        )
        .unwrap();
        let EmuBackend::Pce(backend) = loaded.backend else {
            panic!("PCE loader returned a different backend");
        };
        assert_eq!(
            backend.hucard_board(),
            zeff_pce_core::hardware::PceHuCardBoard::Sf2Ce
        );
        assert_eq!(backend.hucard_rom().len(), 0x28_0000);
    }
}

#[test]
fn pce_loader_applies_board_override_identically_for_direct_and_archive_sources() {
    let mut rom = build_pce_test_rom();
    rom.resize(zeff_pce_core::hardware::POPULOUS_HUCARD_IMAGE_LEN, 0xEA);
    for (source_path, rom_path) in [
        (
            PathBuf::from("synthetic.pce"),
            PathBuf::from("synthetic.pce"),
        ),
        (
            PathBuf::from("synthetic.zip"),
            PathBuf::from("synthetic.zip").join("game.pce"),
        ),
    ] {
        let plain = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &source_path,
            &rom_path,
            Some(rom.clone()),
            BackendLoadConfig::default(),
        )
        .unwrap();
        assert_eq!(plain.backend.save_ram_kind(), SaveRamKind::none());
        let EmuBackend::Pce(plain) = plain.backend else {
            panic!("PCE loader returned a different backend");
        };
        assert_eq!(
            plain.hucard_board(),
            zeff_pce_core::hardware::PceHuCardBoard::Plain
        );

        let mut populous = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &source_path,
            &rom_path,
            Some(rom.clone()),
            BackendLoadConfig {
                pce_hucard_board: Some(zeff_pce_core::hardware::PceHuCardBoard::Populous),
                ..BackendLoadConfig::default()
            },
        )
        .unwrap();
        assert_eq!(
            populous.backend.save_ram_kind(),
            SaveRamKind::mapper_ram_unknown(zeff_pce_core::hardware::POPULOUS_HUCARD_RAM_LEN)
        );
        assert!(!populous.backend.save_ram_kind().is_battery_backed());
        let mut ram = Vec::new();
        assert_eq!(
            populous
                .backend
                .copy_memory_region("save_ram", &mut ram)
                .unwrap(),
            MemoryRegionDescriptor::save_ram(zeff_pce_core::hardware::POPULOUS_HUCARD_RAM_LEN)
        );
        assert_eq!(
            ram,
            vec![0; zeff_pce_core::hardware::POPULOUS_HUCARD_RAM_LEN]
        );
        let EmuBackend::Pce(populous) = populous.backend else {
            panic!("PCE loader returned a different backend");
        };
        assert_eq!(
            populous.hucard_board(),
            zeff_pce_core::hardware::PceHuCardBoard::Populous
        );
    }
}

fn build_pce_263_line_test_rom() -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..13].copy_from_slice(&[
        0xA9, 0xFF, 0x53, 0x01, 0xA9, 0x04, 0x8D, 0x00, 0x04, 0xD4, 0xEA, 0x80, 0xFD,
    ]);
    rom[0x1FFE..0x2000].copy_from_slice(&0xE000_u16.to_le_bytes());
    rom
}

#[test]
fn pce_loader_applies_requested_audio_rate_for_direct_and_archive_sources() {
    let rom = build_pce_263_line_test_rom();
    for (source_path, rom_path) in [
        (PathBuf::from("rate.pce"), PathBuf::from("rate.pce")),
        (
            PathBuf::from("rate.zip"),
            PathBuf::from("rate.zip").join("game.pce"),
        ),
    ] {
        let mut loaded = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &source_path,
            &rom_path,
            Some(rom.clone()),
            BackendLoadConfig {
                sample_rate: Some(48_000),
                ..BackendLoadConfig::default()
            },
        )
        .unwrap();
        let mut frame_counts = Vec::with_capacity(120);
        let mut total_frames = 0;
        for _ in 0..120 {
            loaded.backend.step_frame();
            let mut samples = Vec::new();
            loaded.backend.drain_audio_samples_into(&mut samples);
            let frames = samples.len() / 2;
            frame_counts.push(frames);
            total_frames += frames;
        }

        assert_eq!(total_frames, 96_279);
        assert!(frame_counts.iter().all(|count| matches!(count, 802 | 803)));
    }
}

#[test]
fn pce_loader_rejects_invalid_plain_hucard_lengths() {
    for rom in [
        Vec::new(),
        vec![0; zeff_pce_core::hardware::HUCARD_ROM_REGION_LEN + 1],
        vec![0; zeff_pce_core::hardware::HUCARD_ROM_REGION_LEN + 0x2000],
    ] {
        assert!(
            load_backend_from_rom_source(
                ActiveSystem::Pce,
                &PathBuf::from("invalid.pce"),
                &PathBuf::from("invalid.pce"),
                Some(rom),
                BackendLoadConfig::default(),
            )
            .is_err()
        );
    }

    let header_shaped = vec![0; 0x2000 + 512];
    for (source_path, rom_path) in [
        (PathBuf::from("headered.pce"), PathBuf::from("headered.pce")),
        (
            PathBuf::from("headered.zip"),
            PathBuf::from("headered.zip").join("game.pce"),
        ),
    ] {
        let result = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &source_path,
            &rom_path,
            Some(header_shaped.clone()),
            BackendLoadConfig::default(),
        );
        let Err(error) = result else {
            panic!("header-shaped HuCard image must be rejected");
        };
        assert!(error.to_string().contains("multiple of 8192 bytes"));
    }
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

fn assert_audio_topology_contract(mut backend: EmuBackend, expected_channels: usize) {
    let before = backend
        .audio_topology()
        .expect("audio-capable backend should expose a topology");
    assert!(before.generation > 0);
    assert_eq!(before.channels.len(), expected_channels);
    let ids = before
        .channels
        .iter()
        .map(|channel| channel.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), before.channels.len());
    assert!(
        before
            .channels
            .iter()
            .all(|channel| !channel.name.is_empty() && !channel.group.is_empty())
    );

    backend.step_frame();
    let after = backend
        .audio_topology()
        .expect("audio topology should remain available after stepping");
    assert_eq!(after, before);
    let frame = backend
        .audio_semantic_frame()
        .expect("audio topology should have semantic state");
    assert_eq!(frame.voices.len(), expected_channels);
    let frame_ids = frame
        .voices
        .iter()
        .map(|voice| voice.channel)
        .collect::<BTreeSet<_>>();
    assert_eq!(frame_ids, ids);
    for voice in frame.voices {
        let descriptor = before
            .channels
            .iter()
            .find(|channel| channel.id == voice.channel)
            .expect("semantic voice ID should resolve in the topology");
        if descriptor.class != crate::audio_tooling::AudioVoiceClass::Other {
            assert_eq!(descriptor.class, voice.class);
        }
        assert!(
            descriptor
                .caps
                .contains(crate::audio_tooling::AudioSemanticCaps::GATE)
        );
        if !descriptor
            .caps
            .contains(crate::audio_tooling::AudioSemanticCaps::PITCH)
        {
            assert_eq!(voice.pitch_hz, None);
        }
        if voice.level.is_some() {
            assert!(
                descriptor
                    .caps
                    .contains(crate::audio_tooling::AudioSemanticCaps::LEVEL)
            );
        }
    }
}

#[test]
fn every_audio_backend_exposes_a_stable_topology_contract() {
    assert_audio_topology_contract(build_gb_backend(), 4);
    assert_audio_topology_contract(build_gba_backend(), 6);
    assert_audio_topology_contract(build_nes_backend(), 5);
    assert_audio_topology_contract(build_coleco_backend(), 4);
    assert_audio_topology_contract(build_pce_backend(), 6);
    assert_audio_topology_contract(build_sms_backend(), 4);
    assert_audio_topology_contract(build_ws_backend(), 5);
}

#[test]
fn pce_audio_semantics_keep_zero_pitch_and_wave_noise_identity() {
    let backend = build_pce_backend();
    let topology = backend.audio_topology().unwrap();
    assert_eq!(
        topology.channels[4].class,
        crate::audio_tooling::AudioVoiceClass::WavetableNoise
    );
    assert_eq!(
        topology.channels[5].class,
        crate::audio_tooling::AudioVoiceClass::WavetableNoise
    );

    let frame = backend.audio_semantic_frame().unwrap();
    let expected = (zeff_pce_core::hardware::PSG_CLOCK_NUMERATOR as f64
        / zeff_pce_core::hardware::PSG_CLOCK_DENOMINATOR as f64)
        / (4096.0 * 32.0);
    assert!((frame.voices[0].pitch_hz.unwrap() - expected).abs() < 1e-9);
}

#[test]
fn nes_topology_and_semantic_frame_include_dmc() {
    let mut backend = build_nes_backend();
    let topology = backend.audio_topology().unwrap();
    let dmc = &topology.channels[4];

    assert_eq!(dmc.id, crate::audio_tooling::AudioChannelId(4));
    assert_eq!(dmc.name, "NES DMC");
    assert_eq!(dmc.class, crate::audio_tooling::AudioVoiceClass::Pcm);
    assert_eq!(
        dmc.caps,
        crate::audio_tooling::AudioSemanticCaps::GATE_LEVEL
    );
    assert!(dmc.muteable);

    backend.step_frame();
    let frame = backend
        .audio_semantic_frame()
        .expect("NES should expose semantic audio data for recording/tooling");
    let voice = &frame.voices[4];
    assert_eq!(voice.channel, dmc.id);
    assert_eq!(voice.name, dmc.name);
    assert_eq!(voice.class, dmc.class);
    assert!(!voice.active);
    assert_eq!(voice.pitch_hz, None);
    assert_eq!(voice.level, Some(0.0));
}

#[test]
fn wonder_swan_topology_keeps_hybrid_channel_identities_stable() {
    let topology = build_ws_backend().audio_topology().unwrap();
    assert_eq!(topology.channels[1].name, "WS CH1 Wave/Voice");
    assert_eq!(topology.channels[3].name, "WS CH3 Wave/Noise");
    assert_eq!(
        topology.channels[1].class,
        crate::audio_tooling::AudioVoiceClass::Other
    );
    assert_eq!(
        topology.channels[3].class,
        crate::audio_tooling::AudioVoiceClass::Other
    );
    assert!(
        topology.channels[1]
            .caps
            .contains(crate::audio_tooling::AudioSemanticCaps::PITCH)
    );
    assert!(
        topology.channels[3]
            .caps
            .contains(crate::audio_tooling::AudioSemanticCaps::PITCH)
    );
    assert!(!topology.channels[4].muteable);
    assert!(
        !topology.channels[4]
            .caps
            .contains(crate::audio_tooling::AudioSemanticCaps::PITCH)
    );
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
fn coleco_backend_exposes_semantic_audio_frame_for_recording() {
    let mut backend = build_coleco_backend();
    backend.step_frame();

    let frame = backend
        .audio_semantic_frame()
        .expect("ColecoVision should expose PSG semantic audio data for recording");
    assert_eq!(frame.voices.len(), 4);
    assert_eq!(frame.voices[0].name, "Coleco PSG Tone 0");
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

    let mut coleco_rom = [0; 8 * 1024];
    coleco_rom[..2].copy_from_slice(&[0xAA, 0x55]);
    let mut coleco = zeff_coleco_core::Emulator::new(
        &coleco_rom,
        &[0; zeff_coleco_core::constants::BIOS_SIZE],
        44_100,
    )
    .expect("ColecoVision emulator should initialize");
    assert_debuggable_cpu_byte_access(&mut coleco, 0x6123, 0xBC);
}

#[test]
fn coleco_execution_controls_and_trace_route_through_backend_runtime() {
    let mut backend = build_coleco_backend();
    assert!(backend.coleco().is_some());
    assert!(backend.supports_debugger());
    assert!(backend.supports_execution_controls());
    assert!(backend.supports_opcode_history());
    assert!(backend.supports_guest_calls());

    let mut actions = DebugUiActions::none();
    actions.add_breakpoint = Some(0);
    actions.trace_enabled = Some(true);
    let mut config = BackendRuntimeConfig::new(&actions);
    config.opcode_log_enabled = true;
    backend.apply_runtime_config(config);
    backend.step_frame();
    assert!(backend.is_suspended());

    let actions = DebugUiActions::none();
    let mut config = BackendRuntimeConfig::new(&actions);
    config.opcode_log_enabled = true;
    config.debug_step = true;
    backend.apply_runtime_config(config);

    let EmuBackend::Coleco(coleco) = &backend else {
        panic!("ColecoVision backend changed systems");
    };
    assert_eq!(coleco.emu.cpu().regs().pc, 1);
    assert_eq!(coleco.emu.recent_opcodes(1), vec![(0, 0, 4)]);
    assert_eq!(coleco.emu.instruction_trace().iter().count(), 1);
    assert!(coleco.emu.is_suspended());

    let mut config = BackendRuntimeConfig::new(&actions);
    config.debug_continue = true;
    backend.apply_runtime_config(config);
    assert!(!backend.is_suspended());
}

#[test]
fn coleco_backend_applies_bounded_raw_ram_cheats() {
    let mut backend = build_coleco_backend();
    let capabilities = backend.capabilities();
    assert!(capabilities.supports_cheats);
    assert!(capabilities.cheat_features.supports_ram_writes);
    assert!(!capabilities.cheat_features.supports_rom_patches);
    assert_eq!(capabilities.cheat_features.formats, ["Raw"]);

    backend.apply_ram_cheats(&[CheatPatch::RamWrite {
        address: 0x6123,
        value: CheatValue::Constant(0xA5),
    }]);

    assert_eq!(backend.coleco().unwrap().emu.cpu_peek8(0x6123), 0xA5);
}

#[test]
fn pce_execution_controls_step_and_record_history_through_the_backend_runtime() {
    let mut backend = build_pce_backend();
    assert!(backend.supports_debugger());
    assert!(backend.supports_execution_controls());
    assert!(backend.supports_opcode_history());

    backend.debug_suspend();
    assert!(backend.is_suspended());

    let actions = DebugUiActions::none();
    let mut config = BackendRuntimeConfig::new(&actions);
    config.opcode_log_enabled = true;
    config.debug_step = true;
    backend.apply_runtime_config(config);
    backend.step_frame();

    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert!(pce.is_cpu_suspended());
    assert_eq!(pce.debug_cpu_snapshot().registers().pc, 0xE001);
    let history = pce.recent_opcodes(1);
    assert_eq!(history[0].logical_pc(), 0xE000);
    assert_eq!(history[0].opcode(), 0xD4);

    let mut config = BackendRuntimeConfig::new(&actions);
    config.debug_continue = true;
    backend.apply_runtime_config(config);
    assert!(!backend.is_suspended());
}

#[test]
fn pce_runtime_routes_logical_breakpoint_and_watchpoint_actions() {
    let mut backend = build_pce_backend();
    let mut actions = DebugUiActions::none();
    actions.add_breakpoint = Some(0xE000);
    actions.add_watchpoint = Some((0x4000, 0x400F, WatchType::ReadWrite));
    actions
        .event_breakpoint_changes
        .push((DebugEvent::Interrupt, true));

    backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));
    backend.step_frame();

    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert!(pce.is_cpu_suspended());
    assert_eq!(pce.debug_hit_breakpoint(), Some(0xE000));
    assert_eq!(pce.debug_watchpoints().len(), 1);
    assert_eq!(pce.debug_watchpoints()[0].address, 0x4000);
    assert_eq!(
        pce.iter_event_breakpoints().collect::<Vec<_>>(),
        [DebugEvent::Interrupt]
    );
}

#[test]
fn pce_runtime_routes_trace_configuration_and_dma_events() {
    let mut backend = build_pce_backend();
    backend.debug_suspend();
    let mut actions = DebugUiActions::none();
    actions.trace_enabled = Some(true);
    actions.trace_capacity = Some(3_000);
    actions
        .event_breakpoint_changes
        .extend([(DebugEvent::Interrupt, true), (DebugEvent::Dma, true)]);
    let mut config = BackendRuntimeConfig::new(&actions);
    config.debug_step = true;

    backend.apply_runtime_config(config);
    backend.step_frame();

    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert_eq!(
        pce.iter_event_breakpoints().collect::<Vec<_>>(),
        [DebugEvent::Interrupt, DebugEvent::Dma]
    );
    assert_eq!(pce.instruction_trace().capacity(), 3_000);
    let entries = pce.instruction_trace().iter().collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].mode, TraceExecMode::HuC6280);
    assert_eq!(entries[0].pc, 0xE000);
    assert_eq!(entries[0].instruction_bytes(), &[0xD4]);
}

#[test]
fn pce_runtime_only_collects_waveforms_while_apu_capture_is_requested() {
    let mut backend = build_pce_backend();
    let actions = DebugUiActions::none();
    let mut config = BackendRuntimeConfig::new(&actions);
    config.apu_capture_enabled = true;
    config.skip_audio = true;
    backend.apply_runtime_config(config);
    backend.step_frame();

    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert!(pce.debug_hardware_snapshot().psg.debug_capture_enabled);
    let retained = pce.psg_master_debug_samples_ordered().len();
    assert!(retained > 0);
    assert_eq!(pce.psg_channel_debug_samples_ordered(5).len(), retained);

    backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));
    backend.step_frame();
    let EmuBackend::Pce(pce) = &backend else {
        panic!("PCE backend changed systems");
    };
    assert!(!pce.debug_hardware_snapshot().psg.debug_capture_enabled);
    assert_eq!(pce.psg_master_debug_samples_ordered().len(), retained);
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
    actions.add_one_shot_breakpoint = Some(0xF0010);
    actions.add_breakpoint_after = Some((0xF0020, 4));
    actions
        .event_breakpoint_changes
        .push((DebugEvent::Interrupt, true));
    actions.add_watchpoint = Some((0x0000, 0x000F, WatchType::Write));
    actions.memory_writes.push((0x0000, 0x5A));

    backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));

    let ws = backend
        .ws()
        .expect("backend should remain WonderSwan after debug actions");
    assert_eq!(
        ws.emu.iter_breakpoints().collect::<Vec<_>>(),
        vec![0xF0000, 0xF0010, 0xF0020]
    );
    assert_eq!(
        ws.emu.iter_one_shot_breakpoints().collect::<Vec<_>>(),
        vec![0xF0010]
    );
    assert_eq!(
        ws.emu.iter_breakpoint_hit_conditions().collect::<Vec<_>>()[0].target_hits,
        4
    );
    assert_eq!(
        ws.emu.iter_event_breakpoints().collect::<Vec<_>>(),
        vec![DebugEvent::Interrupt]
    );
    assert_eq!(ws.emu.debug_watchpoints().len(), 1);
    assert_eq!(ws.emu.debug_watchpoints()[0].end_address, 0x000F);
    assert_eq!(
        ws.emu
            .debug_hit_watchpoint()
            .map(|hit| (hit.address, hit.new_value)),
        Some((0x0000, 0x5A))
    );

    let mut actions = DebugUiActions::none();
    actions
        .remove_watchpoints
        .push((0x0000, 0x000F, WatchType::Write));
    backend.apply_runtime_config(BackendRuntimeConfig::new(&actions));
    assert!(
        backend
            .ws()
            .expect("backend should remain WonderSwan")
            .emu
            .debug_watchpoints()
            .is_empty()
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
fn gb_rtc_replay_hash_uses_legacy_v12_projection_and_ignores_wall_clock_timestamp() {
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

    zeff_gb_core::save_state::project_replay_state_bytes(&mut canonicalized_raw).unwrap();
    zeff_gb_core::save_state::canonicalize_replay_hash_bytes(&mut canonicalized_raw);

    assert_eq!(
        replay_hash_state, canonicalized_raw,
        "replay-hash state should match the canonical legacy-v12 projection"
    );
    assert_eq!(
        u32::from_le_bytes(replay_hash_state[8..12].try_into().unwrap()),
        12
    );
}

#[test]
fn gb_replay_start_v13_is_authoritative_and_legacy_v12_remains_loadable() {
    let mut backend = build_gb_backend();
    backend.step_frame();
    let mut seeded_state = backend.encode_state_bytes().unwrap();
    let extension_start = seeded_state
        .windows(4)
        .position(|window| window == b"ZBEX")
        .expect("v13 GB state should contain ZBEX");
    let authoritative_frame = backend.frame_count() + 123;
    seeded_state[extension_start + 8..extension_start + 16]
        .copy_from_slice(&authoritative_frame.to_le_bytes());
    for (index, byte) in seeded_state
        [extension_start + 16..extension_start + 16 + ActiveSystem::GameBoy.framebuffer_len()]
        .iter_mut()
        .enumerate()
    {
        *byte = 0x31_u8.wrapping_add(index as u8);
    }
    backend
        .load_state_from_bytes(seeded_state)
        .expect("seeded authoritative v13 state should load");
    let expected_framebuffer = backend.framebuffer().to_vec();
    assert!(expected_framebuffer.iter().any(|&byte| byte != 0));
    let replay_start = backend
        .encode_replay_start_state_bytes()
        .expect("GB backend should encode a v13 state");
    assert!(
        replay_start == backend.encode_state_bytes().unwrap(),
        "replay-start helper must preserve raw v13 bytes"
    );

    assert_eq!(
        u32::from_le_bytes(replay_start[8..12].try_into().unwrap()),
        13
    );

    let probe = backend
        .probe_replay_state_load(&replay_start, None, false, false)
        .expect("v13 replay start should remain probeable");
    assert_eq!(probe.0, authoritative_frame);
    assert_eq!(probe.1, backend.game_boy_cpu_cycles());
    let mut restored = build_gb_backend();
    restored
        .load_state_from_bytes(replay_start.clone())
        .expect("v13 replay start should remain loadable");
    assert_eq!(restored.frame_count(), authoritative_frame);
    assert!(
        restored.framebuffer() == expected_framebuffer,
        "replay-start framebuffer differs"
    );
    assert_eq!(
        restored.game_boy_cpu_cycles(),
        backend.game_boy_cpu_cycles()
    );

    let mut legacy_start = replay_start;
    zeff_gb_core::save_state::project_replay_state_bytes(&mut legacy_start).unwrap();
    let legacy_probe = backend
        .probe_replay_state_load(&legacy_start, None, false, false)
        .expect("legacy v12 replay start should remain probeable");
    assert_eq!(legacy_probe.1, backend.game_boy_cpu_cycles());
    restored
        .load_state_from_bytes(legacy_start)
        .expect("legacy v12 replay start should remain loadable");
    assert_eq!(
        restored.game_boy_cpu_cycles(),
        backend.game_boy_cpu_cycles()
    );
}

#[test]
fn nes_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_nes_backend(), 1, 2);
}

#[test]
fn nes_replay_start_stays_v11_while_replay_hash_projects_to_v10() {
    let mut backend = build_nes_backend();
    backend.step_frame();
    let raw = backend.encode_state_bytes().unwrap();
    let replay_start = backend.encode_replay_start_state_bytes().unwrap();
    let replay_hash = backend.encode_replay_hash_state_bytes().unwrap();

    assert!(
        replay_start == raw,
        "NES replay start must preserve raw v11"
    );
    assert_eq!(
        u32::from_le_bytes(replay_start[8..12].try_into().unwrap()),
        11
    );
    assert_eq!(
        u32::from_le_bytes(replay_hash[8..12].try_into().unwrap()),
        10
    );
    let mut projected = raw.clone();
    zeff_nes_core::save_state::project_replay_state_bytes(&mut projected).unwrap();
    assert!(
        replay_hash == projected,
        "NES replay hash must use the legacy-v10 projection"
    );

    let expected_frame = backend.frame_count();
    let expected_framebuffer = backend.framebuffer().to_vec();
    let mut restored = build_nes_backend();
    restored.load_state_from_bytes(replay_start).unwrap();
    assert_eq!(restored.frame_count(), expected_frame);
    assert!(
        restored.framebuffer() == expected_framebuffer,
        "NES replay-start framebuffer differs"
    );
}

#[test]
fn pce_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_pce_backend(), 1, 2);
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
fn coleco_backend_replays_save_state_deterministically() {
    assert_save_state_replay_is_deterministic(build_coleco_backend(), 1, 2);
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
fn coleco_runtime_audio_output_settings_do_not_affect_encoded_state() {
    assert_runtime_audio_output_settings_do_not_affect_encoded_state(
        build_coleco_backend(),
        build_coleco_backend(),
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

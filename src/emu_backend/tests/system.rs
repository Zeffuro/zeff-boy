use super::*;

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

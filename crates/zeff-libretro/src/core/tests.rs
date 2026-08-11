use super::*;
use zeff_emu_common::memory::MemoryRegionKind;
use zeff_emu_common::system::{CoreFamily, System};

fn load_sega8(ext: &str) -> CoreState {
    CoreState::from_rom(&[0x76], &format!("test.{ext}")).expect("Sega 8-bit ROM should load")
}

fn ws_rom() -> Vec<u8> {
    let mut rom = vec![0xFF; 0x10000];
    rom[0..2].copy_from_slice(&[0x90, 0xF4]);
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer + 4] = 0x01;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn gba_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom
}

fn nes_rom() -> Vec<u8> {
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

fn gb_rom() -> Vec<u8> {
    vec![0u8; 0x8000]
}

#[test]
fn sega8_extensions_select_expected_systems() {
    let mut sms = load_sega8("sms");
    assert!(matches!(sms.core, ActiveCore::Sega8(_)));
    assert_eq!(sms.system_label(), "SMS");
    assert_eq!(sms.native_width(), 256);
    assert_eq!(sms.native_height(), 192);
    assert_eq!(sms.sram_size(), 0);
    assert_eq!(
        sms.memory_region_size(MemoryRegionKind::PaletteRam),
        zeff_sega8_core::hardware::constants::SMS_CRAM_SIZE
    );
    assert_copyable_regions(&mut sms);

    let mut gg = load_sega8("gg");
    assert!(matches!(gg.core, ActiveCore::Sega8(_)));
    assert_eq!(gg.system_label(), "Game Gear");
    assert_eq!(gg.native_width(), 160);
    assert_eq!(gg.native_height(), 144);
    assert_eq!(gg.sram_size(), 0);
    assert_eq!(
        gg.memory_region_size(MemoryRegionKind::PaletteRam),
        zeff_sega8_core::hardware::constants::SMS_CRAM_SIZE
    );
    assert_copyable_regions(&mut gg);

    for ext in ["sg", "sc"] {
        let mut sg = load_sega8(ext);
        assert!(matches!(sg.core, ActiveCore::Sega8(_)));
        assert_eq!(sg.system_label(), "SG-1000/SC-3000");
        assert_eq!(sg.native_width(), 256);
        assert_eq!(sg.native_height(), 192);
        assert_eq!(sg.sram_size(), 0);
        assert_eq!(sg.memory_region_size(MemoryRegionKind::PaletteRam), 0);
        assert_copyable_regions(&mut sg);
    }
}

#[test]
fn sega8_libretro_uses_path_region_for_video_standard_and_fps() {
    let state =
        CoreState::from_rom(&[0x76], "Example (Europe).sms").expect("Sega 8-bit ROM should load");

    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    assert_eq!(
        emu.video_standard(),
        zeff_sega8_core::hardware::timing::Sega8VideoStandard::Pal
    );
    assert_eq!(
        emu.console_region(),
        zeff_sega8_core::hardware::region::Sega8Region::Export
    );
    assert_eq!(state.fps(), 50.0);
    assert!(state.is_pal_region());
}

#[test]
fn sega8_libretro_forwards_player_two_input() {
    let mut state = load_sega8("sms");

    state.set_input_p2(0x01, 0x04);

    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    let raw = emu
        .bus()
        .input()
        .read_controller(zeff_sega8_core::hardware::input::ControllerPort::Two);
    assert_eq!(raw & (1 << 4), 0, "host A should map to SMS button 1 on P2");
    assert_eq!(raw & (1 << 0), 0, "host Up should map to SMS up on P2");
    assert_ne!(raw & (1 << 5), 0);
}

#[test]
fn sega8_libretro_cheat_set_installs_rom_patches() {
    let mut state = load_sega8("gg");

    state.cheat_set("006-46F");

    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    assert_eq!(emu.rom_patches().len(), 1);
    assert_eq!(emu.cpu_peek8(0x0646), 0x00);

    state.cheat_reset();
    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    assert!(emu.rom_patches().is_empty());
    assert_eq!(emu.cpu_peek8(0x0646), 0x76);
}

#[test]
fn gb_libretro_cheat_set_applies_ram_cheats_each_frame() {
    let rom = gb_rom();
    let mut state = CoreState::from_rom(&rom, "test.gb").expect("GB ROM should load");

    state.cheat_set("C000:42");
    state.step_frame();

    let ActiveCore::Gb(emu) = &state.core else {
        panic!("expected GB core");
    };
    assert_eq!(emu.peek_byte_raw(0xC000), 0x42);

    state.cheat_reset();
    let ActiveCore::Gb(emu) = &mut state.core else {
        panic!("expected GB core");
    };
    emu.write_byte(0xC000, 0x11);

    state.step_frame();

    let ActiveCore::Gb(emu) = &state.core else {
        panic!("expected GB core");
    };
    assert_eq!(emu.peek_byte_raw(0xC000), 0x11);
}

#[test]
fn nes_libretro_cheat_set_applies_raw_ram_cheats_each_frame() {
    let rom = nes_rom();
    let mut state = CoreState::from_rom(&rom, "test.nes").expect("NES ROM should load");

    state.cheat_set("0000:66");
    state.step_frame();

    let ActiveCore::Nes(emu) = &state.core else {
        panic!("expected NES core");
    };
    assert_eq!(emu.cpu_peek(0x0000), 0x66);
}

#[test]
fn sega8_libretro_cheat_set_applies_ram_cheats_each_frame() {
    let mut state = load_sega8("sms");

    state.cheat_set("C000:42");
    state.step_frame();

    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    assert_eq!(emu.cpu_peek8(0xC000), 0x42);
}

#[test]
fn gba_libretro_cheat_set_applies_wide_raw_ram_cheats_each_frame() {
    let rom = gba_rom();
    let mut state = CoreState::from_rom(&rom, "test.gba").expect("GBA ROM should load");

    state.cheat_set("02000000:42");
    state.step_frame();

    let ActiveCore::Gba(emu) = &state.core else {
        panic!("expected GBA core");
    };
    assert_eq!(emu.cpu_peek8(0x0200_0000), 0x42);
}

#[test]
fn ws_libretro_cheat_set_applies_wide_raw_ram_cheats_each_frame() {
    let rom = ws_rom();
    let mut state = CoreState::from_rom(&rom, "test.ws").expect("WonderSwan ROM should load");

    state.cheat_set("00001234:56");
    state.step_frame();

    let ActiveCore::Ws(emu) = &state.core else {
        panic!("expected WS core");
    };
    assert_eq!(emu.cpu_peek8(0x0000_1234), 0x56);
}

#[test]
fn gba_memory_regions_include_debuggable_video_side_regions() {
    let rom = gba_rom();
    let mut state = CoreState::from_rom(&rom, "test.gba").expect("GBA ROM should load");
    let regions = state.memory_regions();

    assert_eq!(
        state.memory_region_size(MemoryRegionKind::PaletteRam),
        zeff_gba_core::hardware::constants::PALETTE_RAM_SIZE
    );
    assert_eq!(
        state.memory_region_size(MemoryRegionKind::Oam),
        zeff_gba_core::hardware::constants::OAM_SIZE
    );
    assert_eq!(
        state.memory_region_size(MemoryRegionKind::IoRegisters),
        zeff_gba_core::hardware::constants::IO_SIZE
    );
    assert!(regions.iter().any(|region| region.id == "palette_ram"));
    assert!(regions.iter().any(|region| region.id == "oam"));
    assert!(regions.iter().any(|region| region.id == "io_registers"));
    assert_copyable_regions(&mut state);
}

#[test]
fn nes_memory_regions_include_ppu_palette_and_oam() {
    let rom = nes_rom();
    let mut state = CoreState::from_rom(&rom, "test.nes").expect("NES ROM should load");

    assert_eq!(state.memory_region_size(MemoryRegionKind::PaletteRam), 32);
    assert_eq!(state.memory_region_size(MemoryRegionKind::Oam), 256);
    assert_eq!(state.memory_region_size(MemoryRegionKind::IoRegisters), 0);
    assert_copyable_regions(&mut state);
}

#[test]
fn gb_memory_regions_are_copyable_by_descriptor() {
    let rom = gb_rom();
    let mut state = CoreState::from_rom(&rom, "test.gb").expect("GB ROM should load");

    assert!(state.memory_region_size(MemoryRegionKind::SystemRam) > 0);
    assert!(state.memory_region_size(MemoryRegionKind::VideoRam) > 0);
    assert_copyable_regions(&mut state);
}

#[test]
fn libretro_valid_extensions_include_gba_and_sega8() {
    let extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");

    for ext in [
        "gb", "gbc", "sgb", "gba", "nes", "ws", "wsc", "sms", "gg", "sg", "sc",
    ] {
        assert!(
            extensions.split('|').any(|entry| entry == ext),
            "missing extension: {ext}"
        );
    }
}

#[test]
fn system_specs_map_to_libretro_core_state() {
    let valid_extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");

    for spec in System::specs() {
        for extension in spec.rom_extensions {
            assert!(
                valid_extensions.split('|').any(|entry| entry == *extension),
                "libretro valid extension list is missing {extension}"
            );

            let rom = rom_for_system(spec.system);
            let path = format!("matrix.{extension}");
            let state = CoreState::from_rom(&rom, &path).unwrap_or_else(|err| {
                panic!(
                    "libretro core should initialize {} ROM {path}: {err}",
                    spec.code
                )
            });

            assert_eq!(active_core_family(&state), spec.core_family);
            assert_eq!(state.native_width(), spec.screen_size.0);
            assert_eq!(state.native_height(), spec.screen_size.1);
            assert_eq!(state.system_label(), expected_system_label(spec.system));
        }
    }
}

#[test]
fn wonderswan_extensions_select_ws_core() {
    let rom = ws_rom();
    for ext in ["ws", "wsc"] {
        let mut state =
            CoreState::from_rom(&rom, &format!("test.{ext}")).expect("WonderSwan ROM should load");

        assert!(matches!(state.core, ActiveCore::Ws(_)));
        assert_eq!(state.system_label(), "WonderSwan");
        assert_eq!(state.native_width(), 224);
        assert_eq!(state.native_height(), 144);
        assert_eq!(state.video_ram_size(), state.system_ram_size());
        assert!(state.video_ram_size() > 0);
        let regions = state.memory_regions();
        assert_eq!(
            regions
                .iter()
                .find(|region| region.id == "cpu")
                .map(|region| region.address_bits),
            Some(Some(20))
        );
        assert!(regions.iter().any(|region| {
            region.kind == MemoryRegionKind::VideoRam && region.size == Some(state.video_ram_size())
        }));
        state.refresh_video_ram();
        assert_eq!(state.video_ram_buf.len(), state.video_ram_size());
        assert_copyable_regions(&mut state);
        assert!(state.encode_state().is_ok());
    }
}

fn rom_for_system(system: System) -> Vec<u8> {
    match system {
        System::Gb => gb_rom(),
        System::Gba => gba_rom(),
        System::Nes => nes_rom(),
        System::Ws => ws_rom(),
        System::Sms | System::Gg | System::Sg => vec![0x76],
    }
}

fn active_core_family(state: &CoreState) -> CoreFamily {
    match &state.core {
        ActiveCore::Gb(_) => CoreFamily::GameBoy,
        ActiveCore::Gba(_) => CoreFamily::GameBoyAdvance,
        ActiveCore::Nes(_) => CoreFamily::Nes,
        ActiveCore::Sega8(_) => CoreFamily::Sega8,
        ActiveCore::Ws(_) => CoreFamily::WonderSwan,
    }
}

fn expected_system_label(system: System) -> &'static str {
    match system {
        System::Gb => "GB/GBC",
        System::Gba => "GBA",
        System::Nes => "NES",
        System::Ws => "WonderSwan",
        System::Sms => "SMS",
        System::Gg => "Game Gear",
        System::Sg => "SG-1000/SC-3000",
    }
}

fn assert_copyable_regions(state: &mut CoreState) {
    let regions = state.memory_regions();
    let mut copied = Vec::new();

    for region in regions {
        if region.kind == MemoryRegionKind::CpuAddressSpace {
            assert!(
                state.copy_memory_region(region.id, &mut copied).is_err(),
                "CPU address spaces should not be copied as finite memory regions"
            );
            continue;
        }

        let copied_region = state
            .copy_memory_region(region.id, &mut copied)
            .unwrap_or_else(|err| {
                panic!(
                    "copying libretro memory region '{}' failed: {err}",
                    region.id
                )
            });
        assert_eq!(copied_region, region);
        assert_eq!(copied.len(), region.size.unwrap_or_default());

        if let Some(alias) = region.aliases.first() {
            let alias_region = state
                .copy_memory_region(alias, &mut copied)
                .unwrap_or_else(|err| {
                    panic!(
                        "copying libretro memory region '{}' through alias '{}' failed: {err}",
                        region.id, alias
                    )
                });
            assert_eq!(alias_region, region);
            assert_eq!(copied.len(), region.size.unwrap_or_default());
        }
    }
}

use super::*;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
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

fn emerald_flash1m_rom() -> Vec<u8> {
    let mut rom = gba_rom();
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    rom.extend_from_slice(b"FLASH1M_V103");
    rom
}

fn gba_backup_rom(marker: &[u8]) -> Vec<u8> {
    let mut rom = gba_rom();
    rom.extend_from_slice(marker);
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

fn pce_rom() -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[0x1FFE..0x2000].copy_from_slice(&0xE000_u16.to_le_bytes());
    rom
}

fn codemasters_rom(bank_count: usize) -> Vec<u8> {
    use zeff_sega8_core::hardware::constants::{
        CODEMASTERS_HEADER_OFFSET, CODEMASTERS_HEADER_SIZE, ROM_BANK_SIZE,
    };

    let mut rom = vec![0; bank_count * ROM_BANK_SIZE];
    for bank in 0..bank_count {
        rom[bank * ROM_BANK_SIZE..(bank + 1) * ROM_BANK_SIZE].fill(bank as u8);
    }

    let offset = CODEMASTERS_HEADER_OFFSET;
    rom[offset] = bank_count as u8;
    rom[offset + 1] = 0x31;
    rom[offset + 2] = 0x08;
    rom[offset + 3] = 0x93;
    rom[offset + 4] = 0x10;
    rom[offset + 5] = 0x59;
    rom[offset + 6..offset + 8].copy_from_slice(&0x1234u16.to_le_bytes());
    rom[offset + 8..offset + 10].copy_from_slice(&0xEDCCu16.to_le_bytes());
    rom[offset + 10..offset + CODEMASTERS_HEADER_SIZE].fill(0);
    rom
}

#[test]
fn sega8_extensions_select_expected_systems() {
    let mut sms = load_sega8("sms");
    assert!(matches!(sms.core, ActiveCore::Sega8(_)));
    assert_eq!(sms.system_label(), "SMS");
    assert_eq!(sms.video_geometry().base_width, 256);
    assert_eq!(sms.video_geometry().base_height, 192);
    assert_eq!(sms.sram_size(), 0);
    assert_eq!(
        sms.memory_region_size(MemoryRegionKind::PaletteRam),
        zeff_sega8_core::hardware::constants::SMS_CRAM_SIZE
    );
    assert_copyable_regions(&mut sms);

    let mut gg = load_sega8("gg");
    assert!(matches!(gg.core, ActiveCore::Sega8(_)));
    assert_eq!(gg.system_label(), "Game Gear");
    assert_eq!(gg.video_geometry().base_width, 160);
    assert_eq!(gg.video_geometry().base_height, 144);
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
        assert_eq!(sg.video_geometry().base_width, 256);
        assert_eq!(sg.video_geometry().base_height, 192);
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
fn sega8_libretro_applies_explicit_mapper_path_tag() {
    let state = CoreState::from_rom(&[0x76], "Example [mapper=janggun].sms")
        .expect("Sega 8-bit ROM should load");

    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    assert_eq!(
        emu.bus().mapper().kind(),
        zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Janggun
    );
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
fn gb_libretro_parameterized_ram_cheats_use_default_value() {
    let rom = gb_rom();
    let mut state = CoreState::from_rom(&rom, "test.gb").expect("GB ROM should load");

    {
        let ActiveCore::Gb(emu) = &mut state.core else {
            panic!("expected GB core");
        };
        emu.write_byte(0xC6A5, 0x7E);
    }

    state.cheat_set("01??A5C6");
    state.step_frame();

    let ActiveCore::Gb(emu) = &state.core else {
        panic!("expected GB core");
    };
    assert_eq!(emu.peek_byte_raw(0xC6A5), 0x00);
}

#[test]
fn gb_libretro_cheat_set_installs_compare_gated_rom_patches() {
    let (patches, _) =
        zeff_gb_core::cheats::parse_cheat("006-CEB-3BE").expect("GB Game Genie should parse");
    let CheatPatch::RomWriteIfEquals {
        address,
        value: CheatValue::Constant(value),
        compare: CheatValue::Constant(compare),
    } = patches[0]
    else {
        panic!("expected compare-gated ROM patch");
    };
    let mut rom = gb_rom();
    rom[usize::from(address)] = compare;
    let mut state = CoreState::from_rom(&rom, "test.gb").expect("GB ROM should load");

    state.cheat_set("006-CEB-3BE");

    let ActiveCore::Gb(emu) = &state.core else {
        panic!("expected GB core");
    };
    assert_eq!(emu.cpu_peek8(address), value);

    state.cheat_reset();
    let ActiveCore::Gb(emu) = &state.core else {
        panic!("expected GB core");
    };
    assert_eq!(emu.cpu_peek8(address), compare);
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
fn nes_libretro_cheat_set_installs_compare_gated_rom_patches() {
    let patch = zeff_nes_core::cheats::decode_nes_game_genie("AAEAAGAE")
        .expect("NES Game Genie should parse");
    let mut rom = nes_rom();
    let prg_offset = 16 + usize::from(patch.address - 0x8000);
    rom[prg_offset] = patch.compare.expect("code should include compare byte");
    let mut state = CoreState::from_rom(&rom, "test.nes").expect("NES ROM should load");

    state.cheat_set("AAEAAGAE");

    let ActiveCore::Nes(emu) = &state.core else {
        panic!("expected NES core");
    };
    assert_eq!(emu.cpu_peek(patch.address), patch.value);

    state.cheat_reset();
    let ActiveCore::Nes(emu) = &state.core else {
        panic!("expected NES core");
    };
    assert_eq!(emu.cpu_peek(patch.address), patch.compare.unwrap());
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
fn sega8_libretro_cheat_set_installs_compare_gated_rom_patches() {
    let mut rom = vec![0; zeff_sega8_core::hardware::constants::ROM_BANK_SIZE * 2];
    rom[0x0646] = 0x04;
    let mut state = CoreState::from_rom(&rom, "test.gg").expect("Sega 8-bit ROM should load");

    state.cheat_set("006-46F-F7A");

    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    assert_eq!(emu.cpu_peek8(0x0646), 0x00);

    state.cheat_reset();
    let ActiveCore::Sega8(emu) = &state.core else {
        panic!("expected Sega8 core");
    };
    assert_eq!(emu.cpu_peek8(0x0646), 0x04);
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
fn gba_libretro_cheat_set_applies_wide_raw_multi_code_each_frame() {
    let rom = gba_rom();
    let mut state = CoreState::from_rom(&rom, "test.gba").expect("GBA ROM should load");

    state.cheat_set("$02000000:42 + 0x02000001 = 43");
    state.step_frame();

    let ActiveCore::Gba(emu) = &state.core else {
        panic!("expected GBA core");
    };
    assert_eq!(emu.cpu_peek8(0x0200_0000), 0x42);
    assert_eq!(emu.cpu_peek8(0x0200_0001), 0x43);
}

#[test]
fn gba_libretro_cheat_set_applies_codebreaker_ram_writes_each_frame() {
    let rom = gba_rom();
    let mut state = CoreState::from_rom(&rom, "test.gba").expect("GBA ROM should load");

    state.cheat_set("3200E924+0096+8201A454+07B7");
    state.step_frame();

    let ActiveCore::Gba(emu) = &state.core else {
        panic!("expected GBA core");
    };
    assert_eq!(emu.cpu_peek8(0x0200_E924), 0x96);
    assert_eq!(emu.cpu_peek8(0x0201_A454), 0xB7);
    assert_eq!(emu.cpu_peek8(0x0201_A455), 0x07);
}

#[test]
fn gba_libretro_cheat_set_decrypts_codebreaker_sequence_across_calls() {
    let rom = gba_rom();
    let mut state = CoreState::from_rom(&rom, "test.gba").expect("GBA ROM should load");
    {
        let ActiveCore::Gba(emu) = &mut state.core else {
            panic!("expected GBA core");
        };
        emu.cpu_write8(0x0200_23BE, 0x12);
        emu.cpu_write8(0x0200_23BF, 0x34);
    }

    state.cheat_set("9F6637CD47C3");
    state.cheat_set("5B1005082B1B");
    state.step_frame();

    let ActiveCore::Gba(emu) = &state.core else {
        panic!("expected GBA core");
    };
    assert_eq!(emu.cpu_peek8(0x0200_23BE), 0x00);
    assert_eq!(emu.cpu_peek8(0x0200_23BF), 0x00);
}

#[test]
fn gba_libretro_ram_cheats_check_existing_value() {
    let rom = gba_rom();
    let mut state = CoreState::from_rom(&rom, "test.gba").expect("GBA ROM should load");

    {
        let ActiveCore::Gba(emu) = &mut state.core else {
            panic!("expected GBA core");
        };
        emu.cpu_write8(0x0200_0000, 0x01);
    }

    state.ram_cheats = vec![CheatPatch::WideRamWriteIfEquals {
        address: 0x0200_0000,
        value: CheatValue::Constant(0x42),
        compare: CheatValue::Constant(0x99),
    }];
    state.apply_ram_cheats();
    let ActiveCore::Gba(emu) = &state.core else {
        panic!("expected GBA core");
    };
    assert_eq!(emu.cpu_peek8(0x0200_0000), 0x01);

    state.ram_cheats = vec![CheatPatch::WideRamWriteIfEquals {
        address: 0x0200_0000,
        value: CheatValue::Constant(0x42),
        compare: CheatValue::Constant(0x01),
    }];
    state.apply_ram_cheats();
    let ActiveCore::Gba(emu) = &state.core else {
        panic!("expected GBA core");
    };
    assert_eq!(emu.cpu_peek8(0x0200_0000), 0x42);
}

#[test]
fn gba_libretro_cheat_reset_clears_wide_raw_ram_cheats() {
    let rom = gba_rom();
    let mut state = CoreState::from_rom(&rom, "test.gba").expect("GBA ROM should load");

    state.cheat_set("02000000:42");
    state.cheat_reset();
    state.step_frame();

    let ActiveCore::Gba(emu) = &state.core else {
        panic!("expected GBA core");
    };
    assert_eq!(emu.cpu_peek8(0x0200_0000), 0x00);
}

#[test]
fn gba_libretro_reset_preserves_backup_and_rtc() {
    let mut state =
        CoreState::from_rom(&emerald_flash1m_rom(), "emerald.gba").expect("GBA ROM should load");
    let date_time =
        zeff_gba_core::hardware::cartridge::RtcDateTime::new(2024, 2, 29, 4, [12, 34, 56])
            .expect("valid RTC date/time");
    let mut backup = vec![0xFF; state.sram_size()];
    backup[0x1234] = 0x5A;

    {
        let ActiveCore::Gba(emu) = &mut state.core else {
            panic!("expected GBA core");
        };
        emu.load_battery_sram(&backup)
            .expect("Flash save should load");
        assert!(emu.set_rtc_date_time(date_time));
    }

    state.reset().unwrap();

    let ActiveCore::Gba(emu) = &state.core else {
        panic!("expected GBA core");
    };
    assert_eq!(emu.dump_battery_sram(), Some(backup));
    assert_eq!(emu.rtc_date_time(), Some(date_time));
}

#[test]
fn gba_sync_sram_to_buf_matches_owned_backup_for_all_kinds() {
    let cases: &[(&str, &[u8])] = &[
        ("none.gba", b""),
        ("sram.gba", b"SRAM_V113"),
        ("flash512.gba", b"FLASH512_V131"),
        ("flash1m.gba", b"FLASH1M_V103"),
        ("eeprom.gba", b"EEPROM_V122"),
    ];

    for &(path, marker) in cases {
        let mut state =
            CoreState::from_rom(&gba_backup_rom(marker), path).expect("GBA ROM should load");
        let expected = state.battery_sram();
        let mut output = expected
            .as_ref()
            .map_or_else(Vec::new, |bytes| vec![0xA5; bytes.len()]);
        if let Some(expected) = &expected {
            let mut data = expected.clone();
            for (index, byte) in data.iter_mut().enumerate() {
                *byte = index as u8;
            }
            state.load_battery_sram(&data).unwrap();
        }

        let expected = state.battery_sram();
        state.sync_sram_to_buf(&mut output).unwrap();
        match expected {
            Some(expected) => assert_eq!(output, expected),
            None => assert!(output.is_empty()),
        }
    }
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
    assert_eq!(
        state.memory_region_size(MemoryRegionKind::ExternalWorkRam),
        zeff_gba_core::hardware::constants::EWRAM_SIZE
    );
    assert_eq!(
        state.memory_region_size(MemoryRegionKind::InternalWorkRam),
        zeff_gba_core::hardware::constants::IWRAM_SIZE
    );
    assert!(regions.iter().any(|region| region.id == "ewram"));
    assert!(regions.iter().any(|region| region.id == "iwram"));
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
fn libretro_valid_extensions_include_registered_systems() {
    let extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");

    for ext in [
        "gb", "gbc", "sgb", "gba", "nes", "fds", "pce", "ws", "wsc", "sms", "gg", "sg", "sc",
    ] {
        assert!(
            extensions.split('|').any(|entry| entry == ext),
            "missing extension: {ext}"
        );
    }
}

#[test]
fn libretro_rejects_deferred_coleco_roms() {
    let extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");
    assert!(!extensions.split('|').any(|entry| entry == "col"));

    let error = CoreState::from_rom(&[0xAA, 0x55], "test.col")
        .err()
        .expect("ColecoVision should have an explicit libretro error");
    assert_eq!(
        error.to_string(),
        "ColecoVision is not available in zeff-libretro"
    );
}

#[test]
fn libretro_registers_pce_hucards_with_fixed_host_geometry() {
    let extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");

    assert!(extensions.split('|').any(|entry| entry == "pce"));
    let state = CoreState::from_rom(&pce_rom(), "test.pce").expect("PCE HuCard should load");
    assert!(matches!(state.core, ActiveCore::Pce(_)));
    assert_eq!(state.system_label(), "PC Engine");
    assert_eq!(state.video_geometry().base_width, 640);
    assert_eq!(state.video_geometry().base_height, 480);
    assert_eq!(state.video_geometry().max_width, 640);
    assert_eq!(state.video_geometry().max_height, 480);
    assert_eq!(state.video_geometry().aspect_ratio, 4.0 / 3.0);
}

#[test]
fn pce_libretro_normalizes_pceas_headers_before_catalog_hashing() {
    let rom = pce_rom();
    let plain = CoreState::from_rom(&rom, "plain.pce").unwrap();
    let mut headered = vec![0; 0x200];
    headered[0] = 1;
    headered.extend_from_slice(&rom);
    let headered = CoreState::from_rom(&headered, "headered.pce").unwrap();

    let (ActiveCore::Pce(plain), ActiveCore::Pce(headered)) = (&plain.core, &headered.core) else {
        panic!("expected PCE hosts");
    };
    assert_eq!(headered.image_sha256(), plain.image_sha256());
    assert_eq!(
        headered.machine().hucard_board(),
        plain.machine().hucard_board()
    );
    assert_eq!(
        headered.machine().hardware_topology(),
        plain.machine().hardware_topology()
    );
}

#[test]
fn pce_libretro_formats_exact_640_by_480_xrgb8888_and_rgb565_frames() {
    let mut state = CoreState::from_rom(&pce_rom(), "video.pce").unwrap();
    state.step_frame();
    let ActiveCore::Pce(host) = &state.core else {
        panic!("expected PCE host");
    };
    let rgba = host.framebuffer().to_vec();

    let xrgb = state.framebuffer_as_xrgb8888().to_vec();
    let rgb565 = state.framebuffer_as_rgb565().to_vec();
    assert_eq!(xrgb.len(), 640 * 480 * 4);
    assert_eq!(rgb565.len(), 640 * 480 * 2);

    for index in [0, 319, 640 * 240, 640 * 480 - 1] {
        let source = &rgba[index * 4..index * 4 + 4];
        assert_eq!(
            &xrgb[index * 4..index * 4 + 4],
            &[source[2], source[1], source[0], 0]
        );
        let expected = (((u16::from(source[0]) >> 3) << 11)
            | ((u16::from(source[1]) >> 2) << 5)
            | (u16::from(source[2]) >> 3))
            .to_le_bytes();
        assert_eq!(&rgb565[index * 2..index * 2 + 2], &expected);
    }
}

#[test]
fn pce_libretro_roundtrips_serialize_payload_above_four_mib() {
    let mut state = CoreState::from_rom(&pce_rom(), "state.pce").unwrap();
    state.set_input(0x0F, 0x09);
    state.step_frame();
    let encoded = state.encode_state().unwrap();
    assert!(encoded.len() > 4 * 1024 * 1024);
    let expected_frame = match &state.core {
        ActiveCore::Pce(host) => host.framebuffer().to_vec(),
        _ => unreachable!(),
    };

    state.reset().unwrap();
    state.load_state(&encoded).unwrap();

    assert_eq!(state.encode_state().unwrap(), encoded);
    let ActiveCore::Pce(host) = &state.core else {
        panic!("expected PCE host");
    };
    assert_eq!(host.framebuffer(), expected_frame);
}

#[test]
fn failed_unserialize_preserves_continuing_session() {
    let mut source = CoreState::from_rom(&gba_rom(), "rollback.gba").unwrap();
    source.set_input(0x03, 0x05);
    source.step_frame();
    let checkpoint = source.encode_state().unwrap();
    let mut control = CoreState::from_rom(&gba_rom(), "rollback.gba").unwrap();
    let mut target = CoreState::from_rom(&gba_rom(), "rollback.gba").unwrap();
    control.load_state(&checkpoint).unwrap();
    target.load_state(&checkpoint).unwrap();
    let before_failed = target.encode_state().unwrap();
    assert_eq!(control.encode_state().unwrap(), before_failed);
    let mut invalid = checkpoint.clone();
    invalid.push(0xA5);

    assert!(target.load_state(&invalid).is_err());
    assert_eq!(target.encode_state().unwrap(), before_failed);
    control.step_frame();
    target.step_frame();
    control.drain_audio();
    target.drain_audio();
    assert_eq!(
        target.encode_state().unwrap(),
        control.encode_state().unwrap()
    );
    assert_eq!(
        target.framebuffer_as_xrgb8888(),
        control.framebuffer_as_xrgb8888()
    );
    assert_eq!(target.audio_buf, control.audio_buf);
}

#[test]
fn pce_libretro_abi_serialization_roundtrips_above_four_mib() {
    let _abi = crate::callbacks::lock(&crate::callbacks::ABI_TEST_LOCK);
    let mut state = CoreState::from_rom(&pce_rom(), "abi-state.pce").unwrap();
    state.step_frame();
    let expected = state.encode_state().unwrap();
    assert!(expected.len() > 4 * 1024 * 1024);
    *crate::callbacks::lock(&crate::callbacks::CORE) = Some(state);
    *crate::callbacks::lock(&crate::callbacks::MAX_SERIALIZE_SIZE) = 0;

    let serialize_size = crate::serialization::retro_serialize_size();
    assert!(serialize_size > expected.len());
    let mut buffer = vec![0; serialize_size];
    assert!(!crate::serialization::retro_serialize(
        buffer.as_mut_ptr().cast(),
        buffer.len() - 1
    ));
    assert!(crate::serialization::retro_serialize(
        buffer.as_mut_ptr().cast(),
        buffer.len()
    ));
    crate::callbacks::lock(&crate::callbacks::CORE)
        .as_mut()
        .unwrap()
        .reset()
        .unwrap();
    assert!(crate::serialization::retro_unserialize(
        buffer.as_ptr().cast(),
        buffer.len()
    ));

    let restored = crate::callbacks::lock(&crate::callbacks::CORE)
        .as_ref()
        .unwrap()
        .encode_state()
        .unwrap();
    assert_eq!(restored, expected);
    *crate::callbacks::lock(&crate::callbacks::CORE) = None;
}

#[test]
fn pce_libretro_forwards_two_button_player_one_input() {
    let mut state = CoreState::from_rom(&pce_rom(), "input.pce").unwrap();
    state.set_input(0x0F, 0x09);

    let ActiveCore::Pce(host) = &state.core else {
        panic!("expected PCE host");
    };
    let zeff_pce_core::hardware::ControllerDevice::TwoButton(pad) =
        host.machine().devices().controller().device()
    else {
        panic!("expected two-button controller");
    };
    let expected = zeff_pce_core::hardware::PadButtons::I
        | zeff_pce_core::hardware::PadButtons::II
        | zeff_pce_core::hardware::PadButtons::SELECT
        | zeff_pce_core::hardware::PadButtons::RUN
        | zeff_pce_core::hardware::PadButtons::RIGHT
        | zeff_pce_core::hardware::PadButtons::DOWN;
    assert_eq!(pad.buttons(), expected);
}

#[test]
fn pce_libretro_applies_logical_ram_cheats() {
    let mut state = CoreState::from_rom(&pce_rom(), "logical-cheat.pce").unwrap();
    let ActiveCore::Pce(host) = &mut state.core else {
        panic!("expected PCE host");
    };
    host.machine_mut()
        .cpu_mut()
        .cpu_mut()
        .set_mapping_register(2, 0xF8);

    state.cheat_set("4005:42");
    state.apply_ram_cheats();

    let ActiveCore::Pce(host) = &state.core else {
        unreachable!();
    };
    assert_eq!(host.machine().mapped_work_ram()[5], 0x42);
}

#[test]
fn pce_libretro_applies_six_digit_physical_work_ram_cheats() {
    let mut state = CoreState::from_rom(&pce_rom(), "physical-cheat.pce").unwrap();

    state.cheat_set("1F2345:66");
    state.apply_ram_cheats();

    let ActiveCore::Pce(host) = &state.core else {
        panic!("expected PCE host");
    };
    assert_eq!(host.machine().mapped_work_ram()[0x345], 0x66);
}

#[test]
fn pce_libretro_cheat_reset_stops_logical_and_physical_writes() {
    let mut state = CoreState::from_rom(&pce_rom(), "reset-cheats.pce").unwrap();
    let ActiveCore::Pce(host) = &mut state.core else {
        panic!("expected PCE host");
    };
    host.machine_mut()
        .cpu_mut()
        .cpu_mut()
        .set_mapping_register(2, 0xF8);
    state.cheat_set("4005:42 + 1F2345:66");
    state.cheat_reset();

    state.apply_ram_cheats();

    let ActiveCore::Pce(host) = &state.core else {
        unreachable!();
    };
    assert_eq!(host.machine().mapped_work_ram()[5], 0);
    assert_eq!(host.machine().mapped_work_ram()[0x345], 0);
}

#[test]
fn system_specs_map_to_libretro_core_state() {
    let valid_extensions = crate::callbacks::VALID_EXTENSIONS
        .to_str()
        .expect("valid extensions should be UTF-8");

    for spec in System::specs() {
        for extension in spec.rom_extensions {
            if spec.system == System::Coleco {
                assert!(
                    !valid_extensions.split('|').any(|entry| entry == *extension),
                    "deferred ColecoVision extension {extension} must not be registered"
                );
                continue;
            }
            if spec.system == System::Pce && *extension != "pce" {
                continue;
            }
            assert!(
                valid_extensions.split('|').any(|entry| entry == *extension),
                "libretro valid extension list is missing {extension}"
            );

            if *extension == "fds" {
                continue;
            }

            let rom = rom_for_system(spec.system);
            let path = format!("matrix.{extension}");
            let state = CoreState::from_rom(&rom, &path).unwrap_or_else(|err| {
                panic!(
                    "libretro core should initialize {} ROM {path}: {err}",
                    spec.code
                )
            });

            assert_eq!(active_core_family(&state), spec.core_family);
            assert_eq!(state.video_geometry().base_width, spec.screen_size.0);
            assert_eq!(state.video_geometry().base_height, spec.screen_size.1);
            assert_eq!(state.system_label(), expected_system_label(spec.system));
            assert_eq!(
                state.system_ram_size(),
                expected_system_ram_size(spec.system),
                "unexpected system RAM size for {}",
                spec.code
            );
        }
    }
}

#[test]
fn video_geometry_matches_registered_system_matrix() {
    for spec in System::specs() {
        for extension in spec.rom_extensions {
            if *extension == "fds"
                || spec.system == System::Coleco
                || spec.system == System::Pce && *extension != "pce"
            {
                continue;
            }

            let rom = rom_for_system(spec.system);
            let state = CoreState::from_rom(&rom, &format!("geometry.{extension}"))
                .unwrap_or_else(|err| panic!("{} geometry fixture failed: {err}", spec.code));
            let geometry = state.video_geometry();

            assert_eq!(
                (geometry.base_width, geometry.base_height),
                spec.screen_size,
                "unexpected base geometry for {}",
                spec.code
            );
            let expected_maximum = if spec.system == System::Pce {
                (640, 480)
            } else {
                (256, 240)
            };
            assert_eq!(
                (geometry.max_width, geometry.max_height),
                expected_maximum,
                "unexpected maximum geometry for {}",
                spec.code
            );
            let expected_aspect = if spec.system == System::Pce {
                4.0 / 3.0
            } else {
                0.0
            };
            assert_eq!(
                geometry.aspect_ratio, expected_aspect,
                "unexpected aspect hint for {}",
                spec.code
            );
        }
    }
}

#[test]
fn codemasters_save_ram_region_uses_mapper_visible_eight_kilobytes() {
    let mut state = CoreState::from_rom(&codemasters_rom(4), "codemasters.sms")
        .expect("Codemasters ROM should load");
    let mut copied = Vec::new();

    let region = state
        .copy_memory_region("save_ram", &mut copied)
        .expect("Codemasters mapper RAM should be copyable");

    assert_eq!(state.save_ram_kind().size(), 0x2000);
    assert_eq!(region.size, Some(0x2000));
    assert_eq!(copied.len(), 0x2000);
}

#[test]
fn wonderswan_extensions_select_ws_core() {
    let rom = ws_rom();
    for ext in ["ws", "wsc"] {
        let mut state =
            CoreState::from_rom(&rom, &format!("test.{ext}")).expect("WonderSwan ROM should load");

        assert!(matches!(state.core, ActiveCore::Ws(_)));
        assert_eq!(state.system_label(), "WonderSwan");
        assert_eq!(state.video_geometry().base_width, 224);
        assert_eq!(state.video_geometry().base_height, 144);
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
        System::Coleco => vec![0xAA, 0x55],
        System::Pce => pce_rom(),
        System::Ws => ws_rom(),
        System::Sms | System::Gg | System::Sg => vec![0x76],
    }
}

fn active_core_family(state: &CoreState) -> CoreFamily {
    match &state.core {
        ActiveCore::Gb(_) => CoreFamily::GameBoy,
        ActiveCore::Gba(_) => CoreFamily::GameBoyAdvance,
        ActiveCore::Nes(_) => CoreFamily::Nes,
        ActiveCore::Pce(_) => CoreFamily::PcEngine,
        ActiveCore::Sega8(_) => CoreFamily::Sega8,
        ActiveCore::Ws(_) => CoreFamily::WonderSwan,
    }
}

fn expected_system_label(system: System) -> &'static str {
    match system {
        System::Gb => "GB/GBC",
        System::Gba => "GBA",
        System::Nes => "NES",
        System::Coleco => "ColecoVision",
        System::Pce => "PC Engine",
        System::Ws => "WonderSwan",
        System::Sms => "SMS",
        System::Gg => "Game Gear",
        System::Sg => "SG-1000/SC-3000",
    }
}

fn expected_system_ram_size(system: System) -> usize {
    match system {
        System::Gb => zeff_gb_core::hardware::types::constants::WRAM_SIZE * 8,
        System::Gba => zeff_gba_core::hardware::constants::SYSTEM_RAM_SIZE,
        System::Nes => zeff_nes_core::hardware::constants::SYSTEM_RAM_SIZE,
        System::Coleco => 1024,
        System::Pce => zeff_pce_core::hardware::WORK_RAM_LEN,
        System::Ws => zeff_ws_core::hardware::constants::WSC_INTERNAL_RAM_SIZE,
        System::Sms | System::Gg => zeff_sega8_core::hardware::constants::SMS_WORK_RAM_SIZE,
        System::Sg => zeff_sega8_core::hardware::constants::SG_WORK_RAM_SIZE,
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

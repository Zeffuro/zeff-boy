use super::*;

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

fn md_cheat_rom(words: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0; 0x400];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    bytes[0x1f0] = b'U';
    for (index, word) in words.iter().enumerate() {
        bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    bytes
}

#[test]
fn md_raw_ram_cheats_reach_guest_reads_and_reset_after_restore() {
    let bytes = md_cheat_rom(&[
        0x13fc, 0x11, 0xff, 0x100, 0x13f9, 0xff, 0x100, 0xff, 0x101, 0x60f4,
    ]);
    let mut state = CoreState::from_rom(&bytes, "cheats.md").unwrap();
    let ActiveCore::Md(emu) = &state.core else {
        panic!("MD core")
    };
    let initial = emu.encode_state().unwrap();
    state.set_indexed_cheat(0, Some("FF0100:A5"));
    state.set_indexed_cheat(1, Some("FF0110:0008"));
    state.set_indexed_cheat(2, Some("FF0102:11+A10003:00"));
    assert_eq!(state.ram_cheats.len(), 3);
    state.step_frame();
    state.step_frame();
    assert!(state.md_runtime_fault.is_none());
    let ActiveCore::Md(emu) = &state.core else {
        panic!("MD core")
    };
    assert_eq!(emu.peek(0xff0101), Some(0xa5));
    assert_eq!(emu.peek(0xff0102), Some(0));
    assert_eq!(emu.peek(0xff0110), Some(0));
    assert_eq!(emu.peek(0xff0111), Some(8));
    state.cheat_reset();
    assert!(state.ram_cheats.is_empty());
    let ActiveCore::Md(emu) = &mut state.core else {
        panic!("MD core")
    };
    emu.decode_state(&initial).unwrap();
    state.step_frame();
    state.step_frame();
    let ActiveCore::Md(emu) = &state.core else {
        panic!("MD core")
    };
    assert_eq!(emu.peek(0xff0101), Some(0x11));
}

#[test]
fn md_ram_cheats_do_not_publish_after_a_runtime_fault() {
    let mut state =
        CoreState::from_rom(&md_cheat_rom(&[0x3039, 0x0040, 0x0000]), "fault.md").unwrap();
    state.set_indexed_cheat(0, Some("FF0100:A5"));
    state.step_frame();
    assert!(state.md_runtime_fault.is_some());
    let ActiveCore::Md(emu) = &state.core else {
        panic!("MD core")
    };
    assert_eq!(emu.peek(0xff0100), Some(0));
    let failed = emu.clone();
    state.step_frame();
    let ActiveCore::Md(emu) = &state.core else {
        panic!("MD core")
    };
    assert_eq!(emu, &failed);
}

#[test]
fn md_rom_cheats_reach_guest_reads_keep_valid_sets_on_rejection_and_survive_reset() {
    let bytes = md_cheat_rom(&[0x3039, 0, 0x380, 0x33c0, 0xff, 0x40, 0x60f2, 0x4e71]);
    let mut state = CoreState::from_rom(&bytes, "rom-cheats.md").unwrap();
    let ActiveCore::Md(emu) = &state.core else {
        panic!("MD core")
    };
    let initial = emu.encode_state().unwrap();
    state.set_indexed_cheat(0, Some("MJBT-AMNA"));
    state.step_frame();
    let ActiveCore::Md(emu) = &state.core else {
        panic!("MD core")
    };
    assert_eq!(emu.peek(0xff0040), Some(0xa5));
    assert_eq!(emu.peek(0xff0041), Some(0x5a));
    state.set_indexed_cheat(1, Some("000380:0008"));
    state.step_frame();
    let ActiveCore::Md(emu) = &state.core else {
        panic!("MD core")
    };
    assert_eq!(emu.peek(0xff0040), Some(0));
    assert_eq!(emu.peek(0xff0041), Some(8));
    let before = emu.clone();
    let patches = state.ram_cheats.clone();
    for invalid in [
        "000400:1234+FF0100:A5",
        "000004:FFFF",
        "000380:FFFF+A10003:00",
    ] {
        state.set_indexed_cheat(1, Some(invalid));
        assert_eq!(state.ram_cheats, patches);
        let ActiveCore::Md(emu) = &state.core else {
            panic!("MD core")
        };
        assert_eq!(emu, &before);
    }
    state.reset().unwrap();
    state.step_frame();
    let ActiveCore::Md(emu) = &state.core else {
        panic!("MD core")
    };
    assert_eq!(emu.peek(0xff0041), Some(8));
    state.cheat_reset();
    let ActiveCore::Md(emu) = &mut state.core else {
        panic!("MD core")
    };
    emu.decode_state(&initial).unwrap();
    state.step_frame();
    let ActiveCore::Md(emu) = &state.core else {
        panic!("MD core")
    };
    assert!(state.ram_cheats.is_empty() && state.md_runtime_fault.is_none());
    assert_eq!(emu.peek(0xff0040), Some(0));
    assert_eq!(emu.peek(0xff0041), Some(0));
}

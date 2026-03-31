use super::*;

#[test]
fn parse_gameshark() {
    let (patches, ty) = parse_cheat("01FF DEC0").unwrap();
    assert_eq!(ty, CheatType::GameShark);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, value } => {
            assert_eq!(address, 0xC0DE);
            assert_eq!(value, CheatValue::Constant(0xFF));
        }
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn parse_gameshark_no_spaces() {
    let (patches, ty) = parse_cheat("010CA2C6").unwrap();
    assert_eq!(ty, CheatType::GameShark);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, value } => {
            assert_eq!(address, 0xC6A2);
            assert_eq!(value, CheatValue::Constant(0x0C));
        }
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn parse_gameshark_parameterized_full_byte() {
    let (patches, ty) = parse_cheat("01??A5C6").unwrap();
    assert_eq!(ty, CheatType::GameShark);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, value } => {
            assert_eq!(address, 0xC6A5);
            assert_eq!(
                value,
                CheatValue::UserParameterized {
                    mask: 0xFF,
                    base: 0x00,
                }
            );
        }
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn parse_gameshark_parameterized_nibble() {
    let (patches, _) = parse_cheat("01?0A5C6").unwrap();
    match patches[0] {
        CheatPatch::RamWrite { value, .. } => {
            assert_eq!(
                value,
                CheatValue::UserParameterized {
                    mask: 0xF0,
                    base: 0x00,
                }
            );
        }
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn parse_gameshark_multi_part() {
    let (patches, ty) = parse_cheat("01FFC0DE+01AAC0DF").unwrap();
    assert_eq!(ty, CheatType::GameShark);
    assert_eq!(patches.len(), 2);
}

#[test]
fn parse_gameshark_91_opcode() {
    let (patches, ty) = parse_cheat("91??C8C6").unwrap();
    assert_eq!(ty, CheatType::GameShark);
    assert_eq!(patches.len(), 1);
    assert!(matches!(patches[0], CheatPatch::RamWrite { .. }));
}

#[test]
fn parse_gameshark_from_libretro_zelda() {
    let (patches, ty) = parse_cheat("010CA2C6").unwrap();
    assert_eq!(ty, CheatType::GameShark);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, value } => {
            assert_eq!(address, 0xC6A2);
            assert_eq!(value, CheatValue::Constant(0x0C));
        }
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn parse_gameshark_91_from_libretro() {
    let (patches, ty) = parse_cheat("9199BAC6").unwrap();
    assert_eq!(ty, CheatType::GameShark);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, value } => {
            assert_eq!(address, 0xC6BA);
            assert_eq!(value, CheatValue::Constant(0x99));
        }
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn parse_raw() {
    let (patches, ty) = parse_cheat("C000:42").unwrap();
    assert_eq!(ty, CheatType::Raw);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, value } => {
            assert_eq!(address, 0xC000);
            assert_eq!(value, CheatValue::Constant(0x42));
        }
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn parse_game_genie_6_digit() {
    let (patches, ty) = parse_cheat("DEF-GHI").unwrap();
    assert_eq!(ty, CheatType::GameGenie);
    assert_eq!(patches.len(), 1);
    assert!(matches!(patches[0], CheatPatch::RomWrite { .. }));
}

#[test]
fn parse_game_genie_9_digit() {
    let (patches, ty) = parse_cheat("DEF-GHI-JKL").unwrap();
    assert_eq!(ty, CheatType::GameGenie);
    assert_eq!(patches.len(), 1);
    assert!(matches!(patches[0], CheatPatch::RomWriteIfEquals { .. }));
}

#[test]
fn parse_game_genie_9_digit_hex_variant() {
    let (patches, ty) = parse_cheat("006-CEB-3BE").unwrap();
    assert_eq!(ty, CheatType::GameGenie);
    assert_eq!(patches.len(), 1);
    assert!(matches!(patches[0], CheatPatch::RomWriteIfEquals { .. }));
}

#[test]
fn parse_game_genie_multi_code_9_digit() {
    let (patches, ty) = parse_cheat("181-5DA-6EA+061-5EA-2AE+001-82A-E62").unwrap();
    assert_eq!(ty, CheatType::GameGenie);
    assert_eq!(patches.len(), 3);
    for patch in &patches {
        assert!(matches!(patch, CheatPatch::RomWriteIfEquals { .. }));
    }
}

#[test]
fn parse_game_genie_multi_code_6_digit() {
    let (patches, ty) = parse_cheat("01B-13B+C3B-14B+5FB-15B").unwrap();
    assert_eq!(ty, CheatType::GameGenie);
    assert_eq!(patches.len(), 3);
    for patch in &patches {
        assert!(matches!(patch, CheatPatch::RomWrite { .. }));
    }
}

#[test]
fn parse_game_genie_multi_code_mixed_lengths() {
    let (patches, ty) = parse_cheat("DEF-GHI+DEF-GHI-JKL").unwrap();
    assert_eq!(ty, CheatType::GameGenie);
    assert_eq!(patches.len(), 2);
    assert!(matches!(patches[0], CheatPatch::RomWrite { .. }));
    assert!(matches!(patches[1], CheatPatch::RomWriteIfEquals { .. }));
}

#[test]
fn parse_game_genie_long_multi_code() {
    let input = "00A-32A-4C5+304-8EB-3BA+007-808-A29+FE4-CCB-190+C32-9DB-801+007-499-19E+00E-3F9-A29+C96-3FB-6E3";
    let (patches, ty) = parse_cheat(input).unwrap();
    assert_eq!(ty, CheatType::GameGenie);
    assert_eq!(patches.len(), 8);
}

#[test]
fn parse_game_genie_6_digit_from_libretro() {
    let (patches, ty) = parse_cheat("1E3-18B").unwrap();
    assert_eq!(ty, CheatType::GameGenie);
    assert_eq!(patches.len(), 1);
    assert!(matches!(patches[0], CheatPatch::RomWrite { .. }));
}

#[test]
fn parse_xploder() {
    let (patches, ty) = parse_cheat("$0D2ACA55").unwrap();
    assert_eq!(ty, CheatType::XPloder);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, value } => {
            assert_eq!(address, 0xCA55);
            assert_eq!(value, CheatValue::Constant(0x2A));
        }
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn parse_xploder_multi_code() {
    let input = "$0D20502A+$0D20932A+$0D20A12A+$0D202C2A+$0D20BD2A+$0D20492A+$0D20AF2A";
    let (patches, ty) = parse_cheat(input).unwrap();
    assert_eq!(ty, CheatType::XPloder);
    assert_eq!(patches.len(), 7);
    for patch in &patches {
        assert!(matches!(patch, CheatPatch::RamWrite { .. }));
    }
}

#[test]
fn parse_xploder_from_libretro() {
    let (patches, ty) = parse_cheat("$0D61C82A").unwrap();
    assert_eq!(ty, CheatType::XPloder);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, value } => {
            assert_eq!(address, 0xC82A);
            assert_eq!(value, CheatValue::Constant(0x61));
        }
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn cheat_value_resolution_and_matching() {
    let masked = CheatValue::from_mask_base_preserve(0xF0, 0x0A);
    assert_eq!(masked.resolve_with_current(0xB7), 0xBA);
    assert!(masked.matches(0x2A));
    assert!(!masked.matches(0x2B));
}

#[test]
fn cheat_value_resolution_and_matching_user_parameterized() {
    let masked = CheatValue::UserParameterized {
        mask: 0xF0,
        base: 0x0A,
    };
    assert_eq!(masked.resolve_with_current(0xB7), 0xBA);
    assert!(masked.matches(0x2A));
    assert!(!masked.matches(0x2B));
}

#[test]
fn cheat_value_constant_display() {
    assert_eq!(CheatValue::Constant(0xFF).display(), "FF");
    assert_eq!(CheatValue::Constant(0x00).display(), "00");
    assert_eq!(CheatValue::Constant(0xAB).display(), "AB");
}

#[test]
fn cheat_value_parameterized_display() {
    let full = CheatValue::UserParameterized {
        mask: 0xFF,
        base: 0x00,
    };
    assert_eq!(full.display(), "??");

    let hi = CheatValue::UserParameterized {
        mask: 0xF0,
        base: 0x0A,
    };
    assert_eq!(hi.display(), "?A");

    let lo = CheatValue::UserParameterized {
        mask: 0x0F,
        base: 0xA0,
    };
    assert_eq!(lo.display(), "A?");
}

#[test]
fn collect_enabled_patches_resolves_user_parameter_value() {
    let cheat = CheatCode {
        name: "Param cheat".to_string(),
        code_text: "01??A5C6".to_string(),
        enabled: true,
        parameter_value: Some(0x3C),
        code_type: CheatType::GameShark,
        patches: vec![CheatPatch::RamWrite {
            address: 0xA5C6,
            value: CheatValue::from_mask_base_user(0xFF, 0x00),
        }],
    };

    let patches = collect_enabled_patches(&[cheat], &[]);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, value } => {
            assert_eq!(address, 0xA5C6);
            assert_eq!(value, CheatValue::Constant(0x3C));
        }
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn collect_enabled_patches_skips_disabled() {
    let cheats = vec![
        CheatCode {
            name: "Disabled".to_string(),
            code_text: "01FFC0DE".to_string(),
            enabled: false,
            parameter_value: None,
            code_type: CheatType::GameShark,
            patches: vec![CheatPatch::RamWrite {
                address: 0xC0DE,
                value: CheatValue::Constant(0xFF),
            }],
        },
        CheatCode {
            name: "Enabled".to_string(),
            code_text: "01AAC0DF".to_string(),
            enabled: true,
            parameter_value: None,
            code_type: CheatType::GameShark,
            patches: vec![CheatPatch::RamWrite {
                address: 0xC0DF,
                value: CheatValue::Constant(0xAA),
            }],
        },
    ];
    let patches = collect_enabled_patches(&cheats, &[]);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, .. } => assert_eq!(address, 0xC0DF),
        _ => panic!("Expected RamWrite"),
    }
}

#[test]
fn collect_enabled_patches_merges_user_and_libretro() {
    let user = vec![CheatCode {
        name: "User".to_string(),
        code_text: "01FFC0DE".to_string(),
        enabled: true,
        parameter_value: None,
        code_type: CheatType::GameShark,
        patches: vec![CheatPatch::RamWrite {
            address: 0xC0DE,
            value: CheatValue::Constant(0xFF),
        }],
    }];
    let libretro = vec![CheatCode {
        name: "Libretro".to_string(),
        code_text: "01AAC0DF".to_string(),
        enabled: true,
        parameter_value: None,
        code_type: CheatType::GameShark,
        patches: vec![CheatPatch::RamWrite {
            address: 0xC0DF,
            value: CheatValue::Constant(0xAA),
        }],
    }];
    let patches = collect_enabled_patches(&user, &libretro);
    assert_eq!(patches.len(), 2);
}

#[test]
fn parse_cht_file_basic() {
    let content = r#"cheats = 2

            cheat0_desc = "Infinite Health"
            cheat0_code = "010CA2C6"
            cheat0_enable = false

            cheat1_desc = "Walk Through Walls"
            cheat1_code = "010033D0"
            cheat1_enable = true
            "#;
    let cheats = parse_cht_file(content);
    assert_eq!(cheats.len(), 2);
    assert_eq!(cheats[0].name, "Infinite Health");
    assert_eq!(cheats[0].code_text, "010CA2C6");
    assert!(!cheats[0].enabled);
    assert_eq!(cheats[1].name, "Walk Through Walls");
    assert!(cheats[1].enabled);
}

#[test]
fn parse_cht_file_game_genie_multi() {
    let content = r#"cheats = 1

            cheat0_desc = "Moon Jump"
            cheat0_code = "181-5DA-6EA+061-5EA-2AE+001-82A-E62"
            cheat0_enable = false
            "#;
    let cheats = parse_cht_file(content);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].patches.len(), 3);
    assert_eq!(cheats[0].code_type, CheatType::GameGenie);
}

#[test]
fn parse_cht_file_xploder() {
    let content = r#"cheats = 1

            cheat0_desc = "Max Health"
            cheat0_code = "$0D61C82A"
            cheat0_enable = false
            "#;
    let cheats = parse_cht_file(content);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].code_type, CheatType::XPloder);
}

#[test]
fn parse_cht_file_skips_empty_code() {
    let content = r#"cheats = 2

            cheat0_desc = "Has code"
            cheat0_code = "01FFC0DE"
            cheat0_enable = false

            cheat1_desc = "No code"
            cheat1_code = ""
            cheat1_enable = false
            "#;
    let cheats = parse_cht_file(content);
    assert_eq!(cheats.len(), 1);
}

#[test]
fn export_cht_file_roundtrip() {
    let original = vec![CheatCode {
        name: "Test Cheat".to_string(),
        code_text: "01FFC0DE".to_string(),
        enabled: true,
        parameter_value: None,
        code_type: CheatType::GameShark,
        patches: vec![CheatPatch::RamWrite {
            address: 0xC0DE,
            value: CheatValue::Constant(0xFF),
        }],
    }];
    let exported = export_cht_file(&original);
    let reimported = parse_cht_file(&exported);
    assert_eq!(reimported.len(), 1);
    assert_eq!(reimported[0].name, "Test Cheat");
    assert_eq!(reimported[0].code_text, "01FFC0DE");
    assert!(reimported[0].enabled);
}

#[test]
fn parse_invalid() {
    assert!(parse_cheat("not a code").is_err());
}

#[test]
fn parse_empty() {
    assert!(parse_cheat("").is_err());
    assert!(parse_cheat("   ").is_err());
}

use super::storage::{cheat_system_dir, read_cheat_file, storage_key};
use super::*;
use crate::emu_backend::ActiveSystem;

#[test]
fn storage_key_prefers_crc32() {
    assert_eq!(
        storage_key(Some("Example Red"), Some(0xD7037C83)),
        Some("D7037C83".to_string())
    );
}

#[test]
fn storage_key_uses_sanitized_title_when_crc_missing() {
    assert_eq!(
        storage_key(Some("Example: Red/Blue?"), None),
        Some("Example_ Red_Blue_".to_string())
    );
}

#[test]
fn load_uses_legacy_paths_when_new_paths_are_empty() {
    let base = std::env::temp_dir().join(format!(
        "zeff-boy-cheats-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos()
    ));
    let key = "D7037C83";

    let root = base.join("cheats").join("gbc");
    let legacy_user = root.join(format!("{key}.cht"));
    let legacy_libretro = root.join("libretro").join(format!("{key}.cht"));

    std::fs::create_dir_all(
        legacy_user
            .parent()
            .expect("legacy user path should have a parent"),
    )
    .expect("should create legacy user directory");
    std::fs::create_dir_all(
        legacy_libretro
            .parent()
            .expect("legacy libretro path should have a parent"),
    )
    .expect("should create legacy libretro directory");

    std::fs::write(&legacy_user, "cheat0_code = \"01FF8000\"\n")
        .expect("should write legacy user cheat file");
    std::fs::write(&legacy_libretro, "cheat0_code = \"01234567\"\n")
        .expect("should write legacy libretro cheat file");

    let new_user = cheat_system_dir(&root, key).join("user.cht");
    let new_libretro = cheat_system_dir(&root, key).join("libretro.cht");

    let user = {
        let cheats = read_cheat_file(&new_user, ActiveSystem::GameBoy);
        if cheats.is_empty() {
            read_cheat_file(&legacy_user, ActiveSystem::GameBoy)
        } else {
            cheats
        }
    };
    let libretro = {
        let cheats = read_cheat_file(&new_libretro, ActiveSystem::GameBoy);
        if cheats.is_empty() {
            read_cheat_file(&legacy_libretro, ActiveSystem::GameBoy)
        } else {
            cheats
        }
    };

    assert_eq!(user.len(), 1);
    assert_eq!(libretro.len(), 1);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn collect_enabled_patches_resolves_user_parameter_values() {
    let cheat = CheatCode {
        name: "Parameterized".to_string(),
        code_text: "01??C000".to_string(),
        enabled: true,
        parameter_value: Some(0xA0),
        code_type: CheatType::GameShark,
        patches: vec![CheatPatch::RamWrite {
            address: 0xC000,
            value: CheatValue::from_mask_base_user(0xF0, 0x05),
        }],
    };

    let patches = collect_enabled_patches(&[cheat], &[]);

    assert_eq!(
        patches,
        vec![CheatPatch::RamWrite {
            address: 0xC000,
            value: CheatValue::constant(0xA5),
        }]
    );
}

#[test]
fn enabled_patch_hash_tracks_resolved_enabled_cheats() {
    let mut cheat = CheatCode {
        name: "Parameterized".to_string(),
        code_text: "01??C000".to_string(),
        enabled: true,
        parameter_value: Some(0xA0),
        code_type: CheatType::GameShark,
        patches: vec![CheatPatch::RamWrite {
            address: 0xC000,
            value: CheatValue::from_mask_base_user(0xF0, 0x05),
        }],
    };

    let hash_a = enabled_patch_hash(&[cheat.clone()], &[]).expect("enabled cheat should hash");
    cheat.parameter_value = Some(0xB0);
    let hash_b = enabled_patch_hash(&[cheat.clone()], &[]).expect("changed cheat should hash");
    cheat.enabled = false;

    assert_ne!(hash_a, hash_b);
    assert_eq!(enabled_patch_hash(&[cheat], &[]), None);
}

#[test]
fn export_cht_file_roundtrips_through_system_parser() {
    let original = vec![CheatCode {
        name: "Sega raw".to_string(),
        code_text: "C123:42".to_string(),
        enabled: true,
        parameter_value: None,
        code_type: CheatType::Raw,
        patches: vec![CheatPatch::RamWrite {
            address: 0xC123,
            value: CheatValue::constant(0x42),
        }],
    }];

    let exported = export_cht_file(&original);
    let imported = parse_cht_file_for_system(&exported, ActiveSystem::MasterSystem);

    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].name, "Sega raw");
    assert!(imported[0].enabled);
    assert_eq!(imported[0].code_type, CheatType::Raw);
    assert_eq!(imported[0].patches, original[0].patches);
}

#[test]
fn parse_cheat_for_system_nes_game_genie_8_letter() {
    let result = parse_cheat_for_system("ALUZVGEI", ActiveSystem::Nes);
    assert!(result.is_ok());
    let (patches, ty) = result.unwrap();
    assert_eq!(ty, CheatType::GameGenie);
    assert_eq!(patches.len(), 1);
}

#[test]
fn parse_cheat_for_system_nes_game_genie_6_letter() {
    let result = parse_cheat_for_system("ZALXZP", ActiveSystem::Nes);
    assert!(result.is_ok());
    let (patches, ty) = result.unwrap();
    assert_eq!(ty, CheatType::GameGenie);
    assert_eq!(patches.len(), 1);
}

#[test]
fn parse_cheat_for_system_nes_multi_code() {
    let result = parse_cheat_for_system("SZULZISA+EUOZIYEI+AVNULGEZ", ActiveSystem::Nes);
    assert!(result.is_ok());
    let (patches, _) = result.unwrap();
    assert_eq!(patches.len(), 3);
}

#[test]
fn parse_cheat_for_system_nes_raw() {
    let result = parse_cheat_for_system("0055:60", ActiveSystem::Nes);
    assert!(result.is_ok());
    let (patches, ty) = result.unwrap();
    assert_eq!(ty, CheatType::Raw);
    assert_eq!(patches.len(), 1);
}

#[test]
fn parse_cheat_for_system_sega8_raw() {
    let result = parse_cheat_for_system("C123:42", ActiveSystem::MasterSystem);
    let (patches, ty) = result.expect("Sega 8-bit raw cheat should parse");
    assert_eq!(ty, CheatType::Raw);
    assert_eq!(patches.len(), 1);
    match patches[0] {
        CheatPatch::RamWrite { address, value } => {
            assert_eq!(address, 0xC123);
            assert_eq!(value, CheatValue::Constant(0x42));
        }
        _ => panic!("expected RAM write"),
    }
}

#[test]
fn parse_cheat_for_system_coleco_raw() {
    let result = parse_cheat_for_system("6000:42", ActiveSystem::Coleco);
    let (patches, ty) = result.expect("ColecoVision raw RAM cheat should parse");
    assert_eq!(ty, CheatType::Raw);
    assert_eq!(
        patches,
        [CheatPatch::RamWrite {
            address: 0x6000,
            value: CheatValue::Constant(0x42),
        }]
    );
}

#[test]
fn parse_cheat_for_system_pce_raw_multi_code() {
    let (patches, ty) = parse_cheat_for_system("$2000:42+0x2001=7F", ActiveSystem::Pce)
        .expect("PCE raw cheats should parse");
    assert_eq!(ty, CheatType::Raw);
    assert_eq!(
        patches,
        [
            CheatPatch::RamWrite {
                address: 0x2000,
                value: CheatValue::Constant(0x42),
            },
            CheatPatch::RamWrite {
                address: 0x2001,
                value: CheatValue::Constant(0x7F),
            },
        ]
    );
}

#[test]
fn parse_cheat_for_system_pce_libretro_physical_ram() {
    let (patches, ty) = parse_cheat_for_system("1f008d:09+1F6098:70", ActiveSystem::Pce)
        .expect("PCE physical RAM cheats should parse");
    assert_eq!(ty, CheatType::Raw);
    assert_eq!(
        patches,
        [
            CheatPatch::WideRamWrite {
                address: 0x1F_008D,
                value: CheatValue::Constant(0x09),
            },
            CheatPatch::WideRamWrite {
                address: 0x1F_6098,
                value: CheatValue::Constant(0x70),
            },
        ]
    );
}

#[test]
fn parse_cheat_for_system_pce_physical_ram_is_bounded() {
    assert!(parse_cheat_for_system("000100:01", ActiveSystem::Pce).is_err());
    assert!(parse_cheat_for_system("1EFFFF:01", ActiveSystem::Pce).is_err());
    assert!(parse_cheat_for_system("1F8000:01", ActiveSystem::Pce).is_err());
    assert!(parse_cheat_for_system("1FE800:01", ActiveSystem::Pce).is_err());
    assert!(parse_cheat_for_system("1F0000F:0C", ActiveSystem::Pce).is_err());
}

#[test]
fn parse_cht_file_for_system_pce_physical_ram() {
    let content = r#"
        cheats = 1
        cheat0_desc = "Synthetic physical RAM write"
        cheat0_code = "1f008d:09+1f0094:99"
        cheat0_enable = false
    "#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::Pce);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].name, "Synthetic physical RAM write");
    assert!(!cheats[0].enabled);
    assert_eq!(cheats[0].patches.len(), 2);
    assert!(
        cheats[0]
            .patches
            .iter()
            .all(|patch| matches!(patch, CheatPatch::WideRamWrite { .. }))
    );
}

#[test]
fn parse_cht_file_rejects_an_entire_malformed_pce_multi_code() {
    let content = r#"
        cheats = 2
        cheat0_desc = "No partial writes"
        cheat0_code = "1f008d:09+1f0000f:0c"
        cheat0_enable = true
        cheat1_desc = "Valid neighbor"
        cheat1_code = "1f0094:99"
        cheat1_enable = true
    "#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::Pce);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].name, "Valid neighbor");
    assert_eq!(cheats[0].patches.len(), 1);
}

#[test]
fn parse_cheat_for_system_sega8_raw_multi_code() {
    let result = parse_cheat_for_system("$C000:01+0xD000=02", ActiveSystem::GameGear);
    let (patches, ty) = result.expect("Sega 8-bit raw multi-code should parse");
    assert_eq!(ty, CheatType::Raw);
    assert_eq!(patches.len(), 2);
    assert!(matches!(
        patches[0],
        CheatPatch::RamWrite {
            address: 0xC000,
            value: CheatValue::Constant(0x01),
        }
    ));
    assert!(matches!(
        patches[1],
        CheatPatch::RamWrite {
            address: 0xD000,
            value: CheatValue::Constant(0x02),
        }
    ));
}

#[test]
fn parse_cheat_for_system_sega8_raw_requires_full_width() {
    assert!(parse_cheat_for_system("C00:01", ActiveSystem::MasterSystem).is_err());
    assert!(parse_cheat_for_system("C000:1", ActiveSystem::MasterSystem).is_err());
}

#[test]
fn parse_cheat_for_system_sega8_action_replay() {
    let result = parse_cheat_for_system("00D2-AA98", ActiveSystem::MasterSystem);
    let (patches, ty) = result.expect("Sega 8-bit Action Replay cheat should parse");
    assert_eq!(ty, CheatType::ActionReplay);
    assert!(matches!(
        patches.as_slice(),
        [CheatPatch::RamWrite {
            address: 0xD2AA,
            value: CheatValue::Constant(0x98)
        }]
    ));
}

#[test]
fn parse_cheat_for_system_sega8_game_genie() {
    let result = parse_cheat_for_system("006-46F-F7A", ActiveSystem::GameGear);
    let (patches, ty) = result.expect("Sega 8-bit Game Genie cheat should parse");
    assert_eq!(ty, CheatType::GameGenie);
    assert!(matches!(
        patches.as_slice(),
        [CheatPatch::RomWriteIfEquals {
            address: 0x0646,
            value: CheatValue::Constant(0x00),
            compare: CheatValue::Constant(0x04)
        }]
    ));
}

#[test]
fn parse_cheat_for_system_gba_raw_wide() {
    let result = parse_cheat_for_system("02000000:42", ActiveSystem::GameBoyAdvance);
    let (patches, ty) = result.expect("GBA raw wide cheat should parse");
    assert_eq!(ty, CheatType::Raw);
    assert!(matches!(
        patches.as_slice(),
        [CheatPatch::WideRamWrite {
            address: 0x0200_0000,
            value: CheatValue::Constant(0x42)
        }]
    ));
}

#[test]
fn parse_cheat_for_system_gba_raw_wide_multi_code() {
    let result = parse_cheat_for_system(
        "$02000000:42 + 0x02000001 = 43",
        ActiveSystem::GameBoyAdvance,
    );
    let (patches, ty) = result.expect("GBA raw wide multi-code should parse");
    assert_eq!(ty, CheatType::Raw);
    assert_eq!(patches.len(), 2);
    assert!(matches!(
        patches[0],
        CheatPatch::WideRamWrite {
            address: 0x0200_0000,
            value: CheatValue::Constant(0x42)
        }
    ));
    assert!(matches!(
        patches[1],
        CheatPatch::WideRamWrite {
            address: 0x0200_0001,
            value: CheatValue::Constant(0x43)
        }
    ));
}

#[test]
fn parse_cheat_for_system_gba_codebreaker_byte_and_halfword_writes() {
    let result =
        parse_cheat_for_system("3200E924+0096+8201A454+07B7", ActiveSystem::GameBoyAdvance);
    let (patches, ty) = result.expect("GBA CodeBreaker/XPloder RAM writes should parse");
    assert_eq!(ty, CheatType::XPloder);
    assert_eq!(
        patches,
        vec![
            CheatPatch::WideRamWrite {
                address: 0x0200_E924,
                value: CheatValue::Constant(0x96),
            },
            CheatPatch::WideRamWrite {
                address: 0x0201_A454,
                value: CheatValue::Constant(0xB7),
            },
            CheatPatch::WideRamWrite {
                address: 0x0201_A455,
                value: CheatValue::Constant(0x07),
            },
        ]
    );
}

#[test]
fn parse_cheat_for_system_gba_codebreaker_serial_halfword_writes() {
    let result =
        parse_cheat_for_system("42035CBE+1388+00000005+0002", ActiveSystem::GameBoyAdvance);
    let (patches, ty) = result.expect("GBA serial RAM writes should parse");
    assert_eq!(ty, CheatType::XPloder);
    assert_eq!(patches.len(), 10);
    assert_eq!(
        patches[0],
        CheatPatch::WideRamWrite {
            address: 0x0203_5CBE,
            value: CheatValue::Constant(0x88),
        }
    );
    assert_eq!(
        patches[9],
        CheatPatch::WideRamWrite {
            address: 0x0203_5CC7,
            value: CheatValue::Constant(0x13),
        }
    );
}

#[test]
fn parse_cheat_for_system_ws_raw_wide() {
    let result = parse_cheat_for_system("00001234=56", ActiveSystem::WonderSwan);
    let (patches, ty) = result.expect("WS raw wide cheat should parse");
    assert_eq!(ty, CheatType::Raw);
    assert!(matches!(
        patches.as_slice(),
        [CheatPatch::WideRamWrite {
            address: 0x0000_1234,
            value: CheatValue::Constant(0x56)
        }]
    ));
}

#[test]
fn parse_cheat_for_system_wide_raw_requires_full_width() {
    assert!(parse_cheat_for_system("2000000:42", ActiveSystem::GameBoyAdvance).is_err());
    assert!(parse_cheat_for_system("02000000:4", ActiveSystem::GameBoyAdvance).is_err());
}

#[test]
fn parse_cheat_for_system_gb_xploder() {
    let result = parse_cheat_for_system("$0D61C82A", ActiveSystem::GameBoy);
    assert!(result.is_ok());
    let (patches, ty) = result.unwrap();
    assert_eq!(ty, CheatType::XPloder);
    assert_eq!(patches.len(), 1);
}

#[test]
fn parse_cht_file_for_system_nes_game_genie() {
    let content = r#"cheats = 2

cheat0_desc = "Jump in Midair"
cheat0_code = "ALUZVGEI"
cheat0_enable = false

cheat1_desc = "Walk Through Blocks"
cheat1_code = "SZULZISA+EUOZIYEI+AVNULGEZ"
cheat1_enable = false
"#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::Nes);
    assert_eq!(cheats.len(), 2);
    assert_eq!(cheats[0].name, "Jump in Midair");
    assert_eq!(cheats[0].patches.len(), 1);
    assert_eq!(cheats[1].name, "Walk Through Blocks");
    assert_eq!(cheats[1].patches.len(), 3);
}

#[test]
fn parse_cht_file_for_system_gbc_xploder() {
    let content = r#"cheats = 2

cheat0_desc = "Infinite Health"
cheat0_code = "$0D61C82A"
cheat0_enable = true

cheat1_desc = "Weapon Slots"
cheat1_code = "$0D20502A+$0D20932A"
cheat1_enable = false
"#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::GameBoy);
    assert_eq!(cheats.len(), 2);
    assert_eq!(cheats[0].code_type, CheatType::XPloder);
    assert_eq!(cheats[0].patches.len(), 1);
    assert!(cheats[0].enabled);
    assert_eq!(cheats[1].code_type, CheatType::XPloder);
    assert_eq!(cheats[1].patches.len(), 2);
}

#[test]
fn parse_cht_file_for_system_gb_skips_invalid_xploder_entry() {
    let content = r#"cheats = 2

cheat0_desc = "Valid"
cheat0_code = "$0D61C82A"
cheat0_enable = true

cheat1_desc = "Broken"
cheat1_code = "$0D61C82"
cheat1_enable = true
"#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::GameBoy);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].name, "Valid");
    assert_eq!(cheats[0].code_type, CheatType::XPloder);
}

#[test]
fn parse_cht_file_for_system_skips_empty_codes() {
    let content = r#"cheats = 2

cheat0_desc = "Has Weapons"
cheat0_code = "005D:FF"
cheat0_enable = false

cheat1_desc = "Unlimited B"
cheat1_code = ""
cheat1_enable = false
"#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::Nes);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].name, "Has Weapons");
}

#[test]
fn parse_cht_file_for_system_sega8_raw() {
    let content = r#"cheats = 1

cheat0_desc = "Infinite Lives"
cheat0_code = "C123:09"
cheat0_enable = true
"#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::MasterSystem);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].name, "Infinite Lives");
    assert_eq!(cheats[0].code_type, CheatType::Raw);
    assert!(cheats[0].enabled);
}

#[test]
fn parse_cht_file_for_system_sega8_action_replay() {
    let content = r#"cheats = 2

cheat0_desc = "Infinite Rings"
cheat0_code = "00D2-AA98"
cheat0_enable = false

cheat1_desc = "Level Modifier"
cheat1_code = "00D2-3EXX"
cheat1_enable = true
"#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::MasterSystem);
    assert_eq!(cheats.len(), 2);
    assert_eq!(cheats[0].name, "Infinite Rings");
    assert_eq!(cheats[0].code_type, CheatType::ActionReplay);
    assert!(!cheats[0].enabled);
    assert_eq!(cheats[1].parameter_value, Some(0));
    assert_eq!(cheats[1].code_type, CheatType::ActionReplay);
    assert!(cheats[1].enabled);
}

#[test]
fn parse_cht_file_for_system_sega8_game_genie() {
    let content = r#"cheats = 1

cheat0_desc = "Immune To Everything"
cheat0_code = "006-46F-F7A"
cheat0_enable = false
"#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::GameGear);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].name, "Immune To Everything");
    assert_eq!(cheats[0].code_type, CheatType::GameGenie);
}

#[test]
fn parse_cht_file_for_system_gba_raw_wide() {
    let content = r#"cheats = 1

cheat0_desc = "Test GBA RAM"
cheat0_code = "02000000:42"
cheat0_enable = true
"#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::GameBoyAdvance);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].name, "Test GBA RAM");
    assert_eq!(cheats[0].code_type, CheatType::Raw);
    assert!(cheats[0].enabled);
}

#[test]
fn parse_cht_file_for_system_gba_codebreaker_encrypted_entries_keep_state() {
    let content = r#"cheats = 2

cheat0_desc = "Activator"
cheat0_code = "9F6637CD47C3"
cheat0_enable = false

cheat1_desc = "No Random Battles"
cheat1_code = "5B1005082B1B"
cheat1_enable = true
"#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::GameBoyAdvance);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].name, "No Random Battles");
    assert_eq!(cheats[0].code_type, CheatType::XPloder);
    assert!(cheats[0].enabled);
    assert_eq!(
        cheats[0].patches,
        vec![
            CheatPatch::WideRamWrite {
                address: 0x0200_23BE,
                value: CheatValue::Constant(0x00),
            },
            CheatPatch::WideRamWrite {
                address: 0x0200_23BF,
                value: CheatValue::Constant(0x00),
            },
        ]
    );
}

#[test]
fn parse_cht_file_for_system_ws_raw_wide() {
    let content = r#"cheats = 1

cheat0_desc = "Test WS RAM"
cheat0_code = "00001234:56"
cheat0_enable = true
"#;
    let cheats = parse_cht_file_for_system(content, ActiveSystem::WonderSwan);
    assert_eq!(cheats.len(), 1);
    assert_eq!(cheats[0].name, "Test WS RAM");
    assert_eq!(cheats[0].code_type, CheatType::Raw);
    assert!(cheats[0].enabled);
}

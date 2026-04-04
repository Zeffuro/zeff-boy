use super::*;

#[test]
fn storage_key_prefers_crc32() {
    assert_eq!(
        storage_key(Some("Pokemon Red"), Some(0xD7037C83)),
        Some("D7037C83".to_string())
    );
}

#[test]
fn storage_key_uses_sanitized_title_when_crc_missing() {
    assert_eq!(
        storage_key(Some("Pokemon: Red/Blue?"), None),
        Some("Pokemon_ Red_Blue_".to_string())
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

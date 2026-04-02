pub(crate) use zeff_emu_common::cheats::{CheatCode, CheatPatch, CheatType, CheatValue};
pub(crate) use zeff_gb_core::cheats::{collect_enabled_patches, export_cht_file, parse_cheat};

use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

pub(crate) fn try_parse_nes_game_genie(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let patch = zeff_nes_core::cheats::decode_nes_game_genie(input)?;
    let cheat_patch = match patch.compare {
        Some(cmp) => CheatPatch::RomWriteIfEquals {
            address: patch.address,
            value: CheatValue::constant(patch.value),
            compare: CheatValue::constant(cmp),
        },
        None => CheatPatch::RomWrite {
            address: patch.address,
            value: CheatValue::constant(patch.value),
        },
    };
    Some((vec![cheat_patch], CheatType::GameGenie))
}

fn try_parse_single_for_system(
    input: &str,
    system: ActiveSystem,
) -> Option<(Vec<CheatPatch>, CheatType)> {
    if let Ok(result) = parse_cheat(input) {
        return Some(result);
    }
    if system == ActiveSystem::Nes {
        return try_parse_nes_game_genie(input);
    }
    None
}

pub(crate) fn parse_cheat_for_system(
    input: &str,
    system: ActiveSystem,
) -> Result<(Vec<CheatPatch>, CheatType), &'static str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Empty cheat code");
    }

    if let Some(result) = try_parse_single_for_system(trimmed, system) {
        return Ok(result);
    }

    let parts: Vec<&str> = trimmed.split('+').collect();
    if parts.len() > 1 {
        let mut all_patches = Vec::new();
        let mut detected_type: Option<CheatType> = None;

        for part in &parts {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((patches, ty)) = try_parse_single_for_system(part, system) {
                detected_type = Some(ty);
                all_patches.extend(patches);
            } else {
                return Err(
                    "Unrecognized format in multi-code. For GB: GameShark, Game Genie, raw. For NES: Game Genie (AAAAAA/AAAAAAAA), raw (AAAA:VV)",
                );
            }
        }

        if let Some(ty) = detected_type
            && !all_patches.is_empty()
        {
            return Ok((all_patches, ty));
        }
    }

    Err(
        "Unrecognized format. For GB: GameShark (01VVAAAA), Game Genie (XXX-YYY), raw (AAAA:VV). For NES: Game Genie (AAAAAA or AAAAAAAA), raw (AAAA:VV)",
    )
}

pub(crate) fn parse_cht_file_for_system(content: &str, system: ActiveSystem) -> Vec<CheatCode> {
    let mut entries: std::collections::HashMap<usize, (Option<String>, Option<String>, bool)> =
        std::collections::HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("cheat")
            && let Some(idx_end) = rest.find('_')
            && let Ok(idx) = rest[..idx_end].parse::<usize>()
        {
            let field = &rest[idx_end + 1..];
            if let Some(value) = field.strip_prefix("desc = ") {
                let value = value.trim().trim_matches('"').to_string();
                entries.entry(idx).or_insert((None, None, false)).0 = Some(value);
            } else if let Some(value) = field.strip_prefix("code = ") {
                let value = value.trim().trim_matches('"').to_string();
                entries.entry(idx).or_insert((None, None, false)).1 = Some(value);
            } else if let Some(value) = field.strip_prefix("enable = ") {
                let enabled = value.trim() == "true";
                entries.entry(idx).or_insert((None, None, false)).2 = enabled;
            }
        }
    }

    let mut indices: Vec<usize> = entries.keys().copied().collect();
    indices.sort_unstable();

    let mut cheats = Vec::new();
    for idx in indices {
        if let Some((desc, code, enabled)) = entries.remove(&idx) {
            let code_text = code.unwrap_or_default();
            if code_text.is_empty() {
                continue;
            }
            let name = desc.unwrap_or_else(|| code_text.clone());

            match parse_cheat_for_system(&code_text, system) {
                Ok((patches, code_type)) => {
                    let parameter_value =
                        patches.iter().copied().find_map(|p| p.default_user_value());
                    cheats.push(CheatCode {
                        name,
                        code_text,
                        enabled,
                        parameter_value,
                        code_type,
                        patches,
                    });
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse cheat '{}': {} (code: {})",
                        name,
                        e,
                        code_text
                    );
                }
            }
        }
    }

    cheats
}

fn sanitize_rom_title(title: &str) -> String {
    title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

fn storage_key(rom_title: Option<&str>, rom_crc32: Option<u32>) -> Option<String> {
    if let Some(crc) = rom_crc32 {
        return Some(format!("{crc:08X}"));
    }
    rom_title
        .map(sanitize_rom_title)
        .filter(|title| !title.trim().is_empty())
}

fn cheats_root_dir(system: ActiveSystem) -> std::path::PathBuf {
    Settings::settings_dir()
        .join("cheats")
        .join(system.storage_subdir())
}

fn cheat_system_dir(root: &std::path::Path, key: &str) -> std::path::PathBuf {
    root.join("libretro").join(key)
}

fn user_cheat_path(system: ActiveSystem, key: &str) -> std::path::PathBuf {
    cheat_system_dir(&cheats_root_dir(system), key).join("user.cht")
}

fn libretro_cheat_path(system: ActiveSystem, key: &str) -> std::path::PathBuf {
    cheat_system_dir(&cheats_root_dir(system), key).join("libretro.cht")
}

fn legacy_user_cheat_path(system: ActiveSystem, key: &str) -> std::path::PathBuf {
    cheats_root_dir(system).join(format!("{key}.cht"))
}

fn legacy_libretro_cheat_path(system: ActiveSystem, key: &str) -> std::path::PathBuf {
    cheats_root_dir(system)
        .join("libretro")
        .join(format!("{key}.cht"))
}

fn read_cheat_file(path: &std::path::Path, system: ActiveSystem) -> Vec<CheatCode> {
    std::fs::read_to_string(path)
        .map(|c| parse_cht_file_for_system(&c, system))
        .unwrap_or_default()
}

fn write_or_remove(path: &std::path::Path, cheats: &[CheatCode]) {
    if cheats.is_empty() {
        if let Err(e) = std::fs::remove_file(path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("failed to remove cheat file {}: {e}", path.display());
        }
        return;
    }
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        log::error!("failed to create cheat directory {}: {e}", parent.display());
        return;
    }
    if let Err(e) = std::fs::write(path, export_cht_file(cheats)) {
        log::error!("failed to write cheat file {}: {e}", path.display());
    }
}

pub(crate) fn save_game_cheats(
    system: ActiveSystem,
    rom_title: Option<&str>,
    rom_crc32: Option<u32>,
    user: &[CheatCode],
    libretro: &[CheatCode],
) {
    let Some(key) = storage_key(rom_title, rom_crc32) else {
        return;
    };
    write_or_remove(&user_cheat_path(system, &key), user);
    write_or_remove(&libretro_cheat_path(system, &key), libretro);
}

pub(crate) fn load_game_cheats(
    system: ActiveSystem,
    rom_title: Option<&str>,
    rom_crc32: Option<u32>,
) -> (Vec<CheatCode>, Vec<CheatCode>) {
    if let Some(key) = storage_key(rom_title, rom_crc32) {
        let user = {
            let path = user_cheat_path(system, &key);
            let cheats = read_cheat_file(&path, system);
            if cheats.is_empty() {
                read_cheat_file(&legacy_user_cheat_path(system, &key), system)
            } else {
                cheats
            }
        };
        let libretro = {
            let path = libretro_cheat_path(system, &key);
            let cheats = read_cheat_file(&path, system);
            if cheats.is_empty() {
                read_cheat_file(&legacy_libretro_cheat_path(system, &key), system)
            } else {
                cheats
            }
        };
        if !user.is_empty() || !libretro.is_empty() {
            return (user, libretro);
        }
    }
    (Vec::new(), Vec::new())
}

#[cfg(test)]
mod tests {
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
        let result =
            parse_cheat_for_system("SZULZISA+EUOZIYEI+AVNULGEZ", ActiveSystem::Nes);
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
}


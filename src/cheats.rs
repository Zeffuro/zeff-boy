pub(crate) use zeff_emu_common::cheats::{CheatCode, CheatPatch, CheatType, CheatValue};
pub(crate) use zeff_gb_core::cheats::{
    collect_enabled_patches, export_cht_file, parse_cheat, parse_cht_file,
};

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

pub(crate) fn parse_cheat_for_system(
    input: &str,
    system: ActiveSystem,
) -> Result<(Vec<CheatPatch>, CheatType), &'static str> {
    if let Ok(result) = parse_cheat(input) {
        return Ok(result);
    }
    if system == ActiveSystem::Nes
        && let Some(result) = try_parse_nes_game_genie(input)
    {
        return Ok(result);
    }
    Err(
        "Unrecognized format. For GB: GameShark (01VVAAAA), Game Genie (XXX-YYY), raw (AAAA:VV). For NES: Game Genie (AAAAAA or AAAAAAAA), raw (AAAA:VV)",
    )
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

fn user_cheat_path(system: ActiveSystem, key: &str) -> std::path::PathBuf {
    Settings::settings_dir()
        .join("cheats")
        .join(system.storage_subdir())
        .join(format!("{key}.cht"))
}

fn libretro_cheat_path(system: ActiveSystem, key: &str) -> std::path::PathBuf {
    Settings::settings_dir()
        .join("cheats")
        .join(system.storage_subdir())
        .join("libretro")
        .join(format!("{key}.cht"))
}

fn read_cheat_file(path: &std::path::Path) -> Vec<CheatCode> {
    std::fs::read_to_string(path)
        .map(|c| parse_cht_file(&c))
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
        let user = read_cheat_file(&user_cheat_path(system, &key));
        let libretro = read_cheat_file(&libretro_cheat_path(system, &key));
        if !user.is_empty() || !libretro.is_empty() {
            return (user, libretro);
        }
    }
    (Vec::new(), Vec::new())
}

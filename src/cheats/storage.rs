use std::path::{Path, PathBuf};

use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

use super::CheatCode;
use super::cht::{export_cht_file, parse_cht_file_for_system};

pub(crate) fn sanitize_rom_title(title: &str) -> String {
    title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

pub(crate) fn storage_key(rom_title: Option<&str>, rom_crc32: Option<u32>) -> Option<String> {
    if let Some(crc) = rom_crc32 {
        return Some(format!("{crc:08X}"));
    }
    rom_title
        .map(sanitize_rom_title)
        .filter(|title| !title.trim().is_empty())
}

fn cheats_root_dir(system: ActiveSystem) -> PathBuf {
    Settings::settings_dir()
        .join("cheats")
        .join(system.storage_subdir())
}

pub(crate) fn cheat_system_dir(root: &Path, key: &str) -> PathBuf {
    root.join("libretro").join(key)
}

fn user_cheat_path(system: ActiveSystem, key: &str) -> PathBuf {
    cheat_system_dir(&cheats_root_dir(system), key).join("user.cht")
}

fn libretro_cheat_path(system: ActiveSystem, key: &str) -> PathBuf {
    cheat_system_dir(&cheats_root_dir(system), key).join("libretro.cht")
}

fn legacy_user_cheat_path(system: ActiveSystem, key: &str) -> PathBuf {
    cheats_root_dir(system).join(format!("{key}.cht"))
}

fn legacy_libretro_cheat_path(system: ActiveSystem, key: &str) -> PathBuf {
    cheats_root_dir(system)
        .join("libretro")
        .join(format!("{key}.cht"))
}

pub(crate) fn read_cheat_file(path: &Path, system: ActiveSystem) -> Vec<CheatCode> {
    std::fs::read_to_string(path)
        .map(|c| parse_cht_file_for_system(&c, system))
        .unwrap_or_default()
}

fn write_or_remove(path: &Path, cheats: &[CheatCode]) {
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

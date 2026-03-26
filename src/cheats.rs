pub(crate) use zeff_gb_core::cheats::*;

use crate::settings::Settings;
use crate::emu_backend::ActiveSystem;

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
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, export_cht_file(cheats));
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

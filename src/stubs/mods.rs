use std::path::{Path, PathBuf};
use crate::emu_backend::ActiveSystem;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ModEntry {
    pub(crate) filename: String,
    pub(crate) enabled: bool,
}

pub(crate) fn mods_dir_for_rom(_system: ActiveSystem, _rom_crc32: u32) -> PathBuf {
    PathBuf::from("mods")
}

pub(crate) fn discover_mods(_dir: &Path) -> Vec<ModEntry> {
    Vec::new()
}

pub(crate) fn load_mod_config(_dir: &Path) -> Vec<ModEntry> {
    Vec::new()
}

pub(crate) fn save_mod_config(_dir: &Path, _entries: &[ModEntry]) {}

pub(crate) fn apply_enabled_mods(_rom_data: &mut Vec<u8>, _dir: &Path, _mods: &[ModEntry]) -> Vec<String> {
    Vec::new()
}


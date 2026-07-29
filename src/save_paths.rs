use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::platform;

pub(crate) fn slot_path(
    system_subdir: &str,
    state_ext: &str,
    rom_hash: [u8; 32],
    slot: u8,
) -> anyhow::Result<PathBuf> {
    if slot > 9 {
        anyhow::bail!("invalid save-state slot {slot} (must be 0–9)");
    }
    let hash_hex = hex_hash(&rom_hash);
    let mut path = platform::save_dir(system_subdir);
    path.push(format!("{hash_hex}_slot{slot}.{state_ext}"));
    Ok(path)
}

pub(crate) fn auto_save_path(system_subdir: &str, state_ext: &str, rom_hash: [u8; 32]) -> PathBuf {
    let hash_hex = hex_hash(&rom_hash);
    let mut path = platform::save_dir(system_subdir);
    path.push(format!("{hash_hex}_auto.{state_ext}"));
    path
}

pub(crate) fn write_state_bytes_to_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    platform::write_save_data(path, bytes)
}

pub(crate) fn backup_state_path(path: &Path) -> PathBuf {
    let backup_ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!("{ext}.bak"))
        .unwrap_or_else(|| "bak".to_string());
    path.with_extension(backup_ext)
}

pub(crate) fn write_state_bytes_to_file_with_backup(
    path: &Path,
    bytes: &[u8],
) -> anyhow::Result<()> {
    if platform::save_data_exists(path) {
        let backup_path = backup_state_path(path);
        if let Some(parent) = backup_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create backup directory: {}", parent.display())
            })?;
        }
        std::fs::copy(path, &backup_path).with_context(|| {
            format!(
                "failed to back up existing save state from {} to {}",
                path.display(),
                backup_path.display()
            )
        })?;
    }

    write_state_bytes_to_file(path, bytes)
}

pub(crate) fn write_sram_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    platform::write_save_data(path, bytes)
}

fn hex_hash(hash: &[u8; 32]) -> String {
    const_hex::encode(hash)
}

pub(crate) fn sram_path_for_rom(rom_path: &Path) -> PathBuf {
    for ancestor in rom_path.ancestors() {
        if ancestor
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        {
            return ancestor.with_extension("sav");
        }
    }
    if let Some(zip_path) = backslash_or_slash_zip_ancestor(rom_path) {
        return zip_path.with_extension("sav");
    }
    rom_path.with_extension("sav")
}

fn backslash_or_slash_zip_ancestor(path: &Path) -> Option<PathBuf> {
    let text = path.as_os_str().to_string_lossy();
    let mut component_start = 0;
    for (index, ch) in text.char_indices() {
        if !matches!(ch, '\\' | '/') {
            continue;
        }
        if component_has_zip_extension(&text[component_start..index]) {
            return Some(PathBuf::from(&text[..index]));
        }
        component_start = index + ch.len_utf8();
    }
    component_has_zip_extension(&text[component_start..]).then(|| PathBuf::from(text.as_ref()))
}

fn component_has_zip_extension(component: &str) -> bool {
    Path::new(component)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

pub(crate) fn flush_battery_sram(
    rom_path: &Path,
    sram_bytes: Option<Vec<u8>>,
) -> anyhow::Result<Option<String>> {
    let Some(bytes) = sram_bytes else {
        return Ok(None);
    };
    let save_path = sram_path_for_rom(rom_path);
    write_sram_file(&save_path, &bytes)?;
    Ok(Some(save_path.display().to_string()))
}

pub(crate) fn try_load_battery_sram(
    rom_path: &Path,
    system_label: &str,
    has_battery: bool,
    load_fn: impl FnOnce(&[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<Option<String>> {
    if !has_battery {
        return Ok(None);
    }
    let save_path = sram_path_for_rom(rom_path);
    let Some(bytes) = platform::read_save_data(&save_path)
        .with_context(|| format!("failed to read {system_label} save {}", save_path.display()))?
    else {
        return Ok(None);
    };
    load_fn(&bytes)?;
    Ok(Some(save_path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sram_for_regular_rom_uses_rom_stem() {
        assert_eq!(
            sram_path_for_rom(Path::new(r"roms\gba\Game.gba")),
            PathBuf::from(r"roms\gba\Game.sav")
        );
    }

    #[test]
    fn sram_for_zipped_rom_uses_archive_stem() {
        assert_eq!(
            sram_path_for_rom(Path::new(r"roms\gba\Game.zip\Inner.gba")),
            PathBuf::from(r"roms\gba\Game.sav")
        );
    }
}

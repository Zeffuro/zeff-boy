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

pub(crate) fn write_sram_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    platform::write_save_data(path, bytes)
}

fn hex_hash(hash: &[u8; 32]) -> String {
    const_hex::encode(hash)
}

pub(crate) fn sram_path_for_rom(rom_path: &Path) -> PathBuf {
    rom_path.with_extension("sav")
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

use std::io::Read;
use std::path::{Path, PathBuf};

use super::ModEntry;
use crate::emu_backend::ActiveSystem;

pub(crate) fn mods_dir_for_rom(system: ActiveSystem, rom_crc32: u32) -> PathBuf {
    mods_root()
        .join(system.storage_subdir())
        .join(format!("{rom_crc32:08x}"))
}

pub(crate) fn discover_mods(dir: &Path) -> Vec<ModEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut mods: Vec<ModEntry> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            let ext = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|s| s.to_ascii_lowercase());
            match ext.as_deref() {
                Some("ips") => has_header(&path, b"PATCH"),
                Some("bps") => has_header(&path, b"BPS1"),
                Some("ups") => has_header(&path, b"UPS1"),
                _ => false,
            }
        })
        .filter_map(|e| {
            e.file_name().to_str().map(|name| ModEntry {
                filename: name.to_string(),
                enabled: false,
            })
        })
        .collect();
    mods.sort_by(|a, b| {
        a.filename
            .to_ascii_lowercase()
            .cmp(&b.filename.to_ascii_lowercase())
    });
    mods
}

pub(crate) fn load_mod_config(dir: &Path) -> Vec<ModEntry> {
    let config_path = dir.join("mods.json");
    let saved: Vec<ModEntry> = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let discovered = discover_mods(dir);

    let mut merged: Vec<ModEntry> = Vec::with_capacity(discovered.len());
    for disc in &discovered {
        let enabled = saved
            .iter()
            .find(|s| s.filename == disc.filename)
            .map(|s| s.enabled)
            .unwrap_or(false);
        merged.push(ModEntry {
            filename: disc.filename.clone(),
            enabled,
        });
    }
    merged
}

pub(crate) fn save_mod_config(dir: &Path, mods: &[ModEntry]) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("Failed to create mods dir {}: {e}", dir.display());
        return;
    }
    let config_path = dir.join("mods.json");
    match serde_json::to_string_pretty(mods) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&config_path, json) {
                log::warn!("Failed to write mod config: {e}");
            }
        }
        Err(e) => log::warn!("Failed to serialize mod config: {e}"),
    }
}

pub(crate) fn apply_enabled_mods(rom: &mut Vec<u8>, dir: &Path, mods: &[ModEntry]) -> Vec<String> {
    let mut warnings = Vec::new();
    for entry in mods.iter().filter(|m| m.enabled) {
        let patch_path = dir.join(&entry.filename);
        match std::fs::read(&patch_path) {
            Ok(patch_data) => {
                let ext = Path::new(&entry.filename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase());
                let result = match ext.as_deref() {
                    Some("bps") => crate::patching::apply_bps_patch(rom, &patch_data).map(|new| {
                        *rom = new;
                    }),
                    Some("ups") => crate::patching::apply_ups_patch(rom, &patch_data).map(|new| {
                        *rom = new;
                    }),
                    _ => crate::patching::apply_ips_patch(rom, &patch_data),
                };
                match result {
                    Ok(()) => log::info!("Applied mod: {}", entry.filename),
                    Err(e) => {
                        let msg = format!("{}: {e}", entry.filename);
                        log::warn!("Mod apply failed: {msg}");
                        warnings.push(msg);
                    }
                }
            }
            Err(e) => {
                let msg = format!("{}: failed to read: {e}", entry.filename);
                log::warn!("Mod apply failed: {msg}");
                warnings.push(msg);
            }
        }
    }
    warnings
}

fn mods_root() -> PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        return config_dir.join("zeff-boy").join("mods");
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mods")
}

fn has_header(path: &Path, magic: &[u8]) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; magic.len()];
    file.read_exact(&mut buf).is_ok() && buf == magic
}

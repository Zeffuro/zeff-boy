use std::path::{Path, PathBuf};

use crate::emu_backend::ActiveSystem;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ModEntry {
    pub(crate) filename: String,
    pub(crate) enabled: bool,
}

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
                Some("ips") => std::fs::read(&path)
                    .map(|data| crate::ips::validate_ips(&data))
                    .unwrap_or(false),
                Some("bps") => std::fs::read(&path)
                    .map(|data| crate::bps::validate_bps(&data))
                    .unwrap_or(false),
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
    mods.sort_by(|a, b| a.filename.to_ascii_lowercase().cmp(&b.filename.to_ascii_lowercase()));
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
                    Some("bps") => crate::bps::apply_bps_patch(rom, &patch_data).map(|new| {
                        *rom = new;
                    }),
                    _ => crate::ips::apply_ips_patch(rom, &patch_data),
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_empty_dir() {
        let dir = std::env::temp_dir().join("zeff_test_mods_empty");
        let _ = std::fs::create_dir_all(&dir);
        let mods = discover_mods(&dir);
        assert!(
            mods.is_empty()
                || mods
                    .iter()
                    .all(|m| m.filename.ends_with(".ips") || m.filename.ends_with(".bps"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_finds_ips_and_bps_files() {
        let dir = std::env::temp_dir().join("zeff_test_mods_discover_both");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("patch_a.ips"), b"PATCHEOF").unwrap();
        std::fs::write(dir.join("patch_b.IPS"), b"PATCHEOF").unwrap();
        std::fs::write(dir.join("patch_c.bps"), make_test_bps(&[0; 4], &[0; 4])).unwrap();
        std::fs::write(dir.join("readme.txt"), b"not a patch").unwrap();
        let mods = discover_mods(&dir);
        let names: Vec<&str> = mods.iter().map(|m| m.filename.as_str()).collect();
        assert!(names.contains(&"patch_a.ips"));
        assert!(names.contains(&"patch_b.IPS"));
        assert!(names.contains(&"patch_c.bps"));
        assert!(!names.iter().any(|n| n.contains("readme")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_save_roundtrip() {
        let dir = std::env::temp_dir().join("zeff_test_mods_roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("hack.ips"), b"PATCHEOF").unwrap();

        let mut mods = load_mod_config(&dir);
        assert_eq!(mods.len(), 1);
        assert!(!mods[0].enabled);

        mods[0].enabled = true;
        save_mod_config(&dir, &mods);

        let reloaded = load_mod_config(&dir);
        assert_eq!(reloaded.len(), 1);
        assert!(reloaded[0].enabled);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_enabled_mods_applies_ips_patches() {
        let dir = std::env::temp_dir().join("zeff_test_mods_apply_ips");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        let mut patch = Vec::new();
        patch.extend_from_slice(b"PATCH");
        patch.extend_from_slice(&[0x00, 0x00, 0x02, 0x00, 0x01, 0xFF]);
        patch.extend_from_slice(b"EOF");
        std::fs::write(dir.join("test.ips"), &patch).unwrap();

        let entries = vec![ModEntry {
            filename: "test.ips".to_string(),
            enabled: true,
        }];
        let mut rom = vec![0u8; 16];
        let warnings = apply_enabled_mods(&mut rom, &dir, &entries);
        assert!(warnings.is_empty());
        assert_eq!(rom[2], 0xFF);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_enabled_mods_applies_bps_patches() {
        let dir = std::env::temp_dir().join("zeff_test_mods_apply_bps");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        let source = vec![0u8; 8];
        let target = vec![0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x00, 0x00, 0x00];
        let patch = make_test_bps(&source, &target);
        std::fs::write(dir.join("test.bps"), &patch).unwrap();

        let entries = vec![ModEntry {
            filename: "test.bps".to_string(),
            enabled: true,
        }];
        let mut rom = source;
        let warnings = apply_enabled_mods(&mut rom, &dir, &entries);
        assert!(warnings.is_empty());
        assert_eq!(rom, target);

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn make_test_bps(source: &[u8], target: &[u8]) -> Vec<u8> {
        fn encode_varint(mut value: u64) -> Vec<u8> {
            let mut buf = Vec::new();
            loop {
                let mut byte = (value & 0x7f) as u8;
                value >>= 7;
                if value == 0 {
                    byte |= 0x80;
                    buf.push(byte);
                    break;
                }
                buf.push(byte);
                value -= 1;
            }
            buf
        }

        let mut patch = Vec::new();
        patch.extend_from_slice(b"BPS1");
        patch.extend(encode_varint(source.len() as u64));
        patch.extend(encode_varint(target.len() as u64));
        patch.extend(encode_varint(0));

        let cmd = ((target.len() as u64 - 1) << 2) | 1;
        patch.extend(encode_varint(cmd));
        patch.extend_from_slice(target);

        let source_crc = crc32fast::hash(source);
        let target_crc = crc32fast::hash(target);
        patch.extend_from_slice(&source_crc.to_le_bytes());
        patch.extend_from_slice(&target_crc.to_le_bytes());
        let patch_crc = crc32fast::hash(&patch);
        patch.extend_from_slice(&patch_crc.to_le_bytes());
        patch
    }
}


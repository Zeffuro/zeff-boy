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
                Some("ppf") => {
                    has_header(&path, b"PPF10")
                        || has_header(&path, b"PPF20")
                        || has_header(&path, b"PPF30")
                }
                Some("xdelta" | "xdelta3" | "vcdiff") => has_header(&path, b"\xD6\xC3\xC4\0"),
                _ => false,
            }
        })
        .filter_map(|e| {
            e.file_name().to_str().map(|name| ModEntry {
                filename: name.to_string(),
                enabled: false,
                target: None,
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
    for saved in saved {
        if discovered
            .iter()
            .any(|entry| entry.filename == saved.filename)
        {
            merged.push(saved);
        }
    }
    for discovered in discovered {
        if !merged
            .iter()
            .any(|entry| entry.filename == discovered.filename)
        {
            merged.push(discovered);
        }
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

pub(crate) fn mod_advisories(dir: &Path, mods: &[ModEntry]) -> Vec<String> {
    mods.iter()
        .filter(|entry| entry.enabled)
        .filter(|entry| {
            Path::new(&entry.filename)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("ppf"))
        })
        .filter_map(|entry| {
            let patch = std::fs::read(dir.join(&entry.filename)).ok()?;
            (!crate::patching::ppf_has_source_validation(&patch)).then(|| {
                format!(
                    "{} has no source check; verify the exact disc revision",
                    entry.filename
                )
            })
        })
        .collect()
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
                    Some("ppf") => crate::patching::apply_ppf_patch(rom, &patch_data),
                    Some("xdelta" | "xdelta3" | "vcdiff") => {
                        crate::patching::apply_xdelta_patch(rom, &patch_data).map(|new| *rom = new)
                    }
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

pub(crate) fn apply_enabled_pce_cd_mods(
    files: &mut [Vec<u8>],
    file_references: &[String],
    dir: &Path,
    mods: &[ModEntry],
) -> Vec<String> {
    let mut warnings = Vec::new();
    for entry in mods.iter().filter(|entry| entry.enabled) {
        let extension = Path::new(&entry.filename)
            .extension()
            .and_then(|extension| extension.to_str());
        if !extension.is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "ppf" | "xdelta" | "xdelta3" | "vcdiff"
            )
        }) {
            let message = format!(
                "{}: PC Engine CD sets require a PPF or xdelta patch",
                entry.filename
            );
            log::warn!("Mod apply failed: {message}");
            warnings.push(message);
            continue;
        }

        let patch_path = dir.join(&entry.filename);
        let mut missing_ppf_validation = false;
        let result = std::fs::read(&patch_path)
            .map_err(anyhow::Error::from)
            .and_then(|patch| {
                if extension.is_some_and(|extension| extension.eq_ignore_ascii_case("ppf")) {
                    missing_ppf_validation = !crate::patching::ppf_has_source_validation(&patch);
                    crate::patching::apply_ppf_patch_segments(files, &patch)
                } else {
                    let index = xdelta_target_index(entry, file_references)?;
                    let patched = crate::patching::apply_xdelta_patch(&files[index], &patch)?;
                    files[index] = patched;
                    Ok(())
                }
            });
        match result {
            Ok(()) => {
                log::info!("Applied PC Engine CD mod: {}", entry.filename);
                if missing_ppf_validation {
                    let message = format!(
                        "{}: patch has no source check; verify the exact disc revision",
                        entry.filename
                    );
                    log::warn!("Mod warning: {message}");
                    warnings.push(message);
                }
            }
            Err(error) => {
                let message = format!("{}: {error}", entry.filename);
                log::warn!("Mod apply failed: {message}");
                warnings.push(message);
            }
        }
    }
    warnings
}

fn xdelta_target_index(entry: &ModEntry, references: &[String]) -> anyhow::Result<usize> {
    if references.len() == 1 {
        return Ok(0);
    }

    let hint = entry
        .target
        .as_deref()
        .filter(|target| !target.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| infer_track_hint(&entry.filename));
    let Some(hint) = hint else {
        anyhow::bail!(
            "xdelta target is ambiguous; include Track NN in the patch filename or set target in mods.json"
        );
    };
    let normalized_hint = normalize_target(&hint);
    let matches = references
        .iter()
        .enumerate()
        .filter(|(_, reference)| normalize_target(reference).contains(&normalized_hint))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    anyhow::ensure!(
        matches.len() == 1,
        "xdelta target '{hint}' did not match one CUE file"
    );
    Ok(matches[0])
}

fn infer_track_hint(filename: &str) -> Option<String> {
    let lower = filename.to_ascii_lowercase();
    let start = lower.find("track")? + "track".len();
    let digits = lower[start..]
        .trim_start_matches(|character: char| !character.is_ascii_digit())
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty()).then(|| format!("track{digits}"))
}

fn normalize_target(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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

use std::path::{Path, PathBuf};

use zeff_emu_common::system::System;

pub(super) fn discover_symbol_sidecar(source_path: &Path, rom_path: &Path) -> Option<PathBuf> {
    discover_sidecar_with_extension(source_path, rom_path, "sym")
}

pub(super) fn discover_elf_sidecar(source_path: &Path, rom_path: &Path) -> Option<PathBuf> {
    discover_sidecar_with_extension(source_path, rom_path, "elf")
        .or_else(|| discover_sidecar_with_extension(source_path, rom_path, "axf"))
}

pub(super) fn discover_zdbg_sidecar(source_path: &Path, rom_path: &Path) -> Option<PathBuf> {
    discover_sidecar_with_extension(source_path, rom_path, "zdbg.json")
}

pub(super) fn discover_map_sidecar(source_path: &Path, rom_path: &Path) -> Option<PathBuf> {
    discover_sidecar_with_extension(source_path, rom_path, "map")
}

pub(super) fn discover_dbg_sidecar(source_path: &Path, rom_path: &Path) -> Option<PathBuf> {
    discover_sidecar_with_extension(source_path, rom_path, "dbg")
}

pub(super) fn discover_namelist_sidecars(
    system: System,
    source_path: &Path,
    rom_path: &Path,
) -> Vec<PathBuf> {
    if system != System::Nes {
        return Vec::new();
    }
    let (Some(parent), Some(file_name)) = (source_path.parent(), rom_path.file_name()) else {
        return Vec::new();
    };
    let Some(file_name) = file_name.to_str() else {
        return Vec::new();
    };
    let prefix = format!("{}.", file_name.to_ascii_lowercase());
    let mut paths = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        let name = name.to_ascii_lowercase();
                        name.starts_with(&prefix) && name.ends_with(".nl")
                    })
        })
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths
}

fn discover_sidecar_with_extension(
    source_path: &Path,
    rom_path: &Path,
    extension: &str,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if rom_path.is_absolute() || rom_path.exists() {
        candidates.push(rom_path.with_extension(extension));
    }
    if let (Some(parent), Some(file_name)) = (source_path.parent(), rom_path.file_name()) {
        candidates.push(parent.join(file_name).with_extension(extension));
    }
    candidates.push(source_path.with_extension(extension));

    for candidate in &candidates {
        if candidate.is_file() {
            return Some(candidate.clone());
        }
    }

    #[cfg(windows)]
    {
        None
    }

    #[cfg(not(windows))]
    {
        for candidate in candidates {
            let Some(parent) = candidate.parent() else {
                continue;
            };
            let Some(wanted) = candidate.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(entries) = std::fs::read_dir(parent) else {
                continue;
            };
            if let Some(path) = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    path.is_file()
                        && path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
                })
            {
                return Some(path);
            }
        }
        None
    }
}

pub(super) fn user_symbol_sidecar_path(source_path: &Path, rom_path: &Path) -> PathBuf {
    if rom_path.is_absolute() || rom_path.exists() {
        return rom_path.with_extension("user.zdbg.json");
    }
    if let (Some(parent), Some(file_name)) = (source_path.parent(), rom_path.file_name()) {
        return parent.join(file_name).with_extension("user.zdbg.json");
    }
    source_path.with_extension("user.zdbg.json")
}

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::emu_backend::ActiveSystem;

pub(super) fn is_zip_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn is_native_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("7z") || extension.eq_ignore_ascii_case("rar")
        })
}

pub(crate) fn detect_and_extract_rom(
    path: &Path,
) -> anyhow::Result<(PathBuf, Option<Vec<u8>>, ActiveSystem)> {
    let (rom_path, preloaded_data) = if is_zip_path(path) {
        let (virtual_path, data) = super::super::extract_rom_from_zip(path)
            .with_context(|| format!("Failed to extract ROM from '{}'", path.display()))?;
        log::info!(
            "Extracted ROM '{}' ({} bytes) from ZIP",
            virtual_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            data.len()
        );
        (virtual_path, Some(data))
    } else if !path.exists() {
        anyhow::bail!(
            "File not found: '{}'. Check that the path is correct.",
            path.display()
        );
    } else {
        (path.to_path_buf(), None)
    };
    detect_system_for_loaded_path(rom_path, preloaded_data)
}

pub(super) fn detect_and_extract_archive_entry(
    archive_path: &Path,
    entry_index: usize,
) -> anyhow::Result<(PathBuf, Option<Vec<u8>>, ActiveSystem)> {
    let (rom_path, data) = super::super::extract_rom_entry_from_zip(archive_path, entry_index)
        .with_context(|| format!("Failed to extract ROM from '{}'", archive_path.display()))?;
    detect_system_for_loaded_path(rom_path, Some(data))
}

pub(super) fn detect_and_extract_archive_entry_path(
    archive_path: &Path,
    virtual_rom_path: &Path,
) -> anyhow::Result<(PathBuf, Option<Vec<u8>>, ActiveSystem)> {
    let (rom_path, data) =
        super::super::extract_rom_entry_path_from_zip(archive_path, virtual_rom_path)
            .with_context(|| format!("Failed to extract ROM from '{}'", archive_path.display()))?;
    detect_system_for_loaded_path(rom_path, Some(data))
}

fn detect_system_for_loaded_path(
    rom_path: PathBuf,
    preloaded_data: Option<Vec<u8>>,
) -> anyhow::Result<(PathBuf, Option<Vec<u8>>, ActiveSystem)> {
    let system = ActiveSystem::from_path(&rom_path).ok_or_else(|| {
        let ext = rom_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("(none)");
        anyhow::anyhow!(
            "Unsupported file type '.{ext}'. Supported extensions: {}",
            ActiveSystem::supported_extensions()
        )
    })?;
    Ok((rom_path, preloaded_data, system))
}

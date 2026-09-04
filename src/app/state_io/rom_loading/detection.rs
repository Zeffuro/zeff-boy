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

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ZipMediaRoute {
    Generic,
    PceCd,
    SelectionRequired,
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn zip_media_route(
    path: &Path,
    expected_rom_path: Option<&Path>,
) -> anyhow::Result<ZipMediaRoute> {
    if !is_zip_path(path) {
        return Ok(ZipMediaRoute::Generic);
    }
    let contains_cue = crate::emu_backend::pce_cd_zip::zip_contains_cue(path)?;
    if let Some(expected) = expected_rom_path {
        return Ok(
            if expected
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("cue"))
            {
                ZipMediaRoute::PceCd
            } else {
                ZipMediaRoute::Generic
            },
        );
    }
    if !contains_cue {
        return Ok(ZipMediaRoute::Generic);
    }
    let mixed_media = super::super::list_rom_entries_in_zip(path)?
        .iter()
        .any(|entry| {
            !Path::new(&entry.name)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("cue"))
        });
    Ok(if mixed_media {
        ZipMediaRoute::SelectionRequired
    } else {
        ZipMediaRoute::PceCd
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn mixed_pce_zip_uses_the_explicit_member_or_requires_selection() -> anyhow::Result<()> {
        let directory = crate::test_support::test_directory("app-mixed-pce-zip-routing")?;
        let archive = directory.path().join("mixed.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive)?);
        for (name, bytes) in [
            ("game.pce", &[0; 64][..]),
            (
                "disc/disc.cue",
                &b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n"[..],
            ),
            ("disc/disc.bin", &[0; 2048][..]),
        ] {
            writer.start_file(name, zip::write::SimpleFileOptions::default())?;
            writer.write_all(bytes)?;
        }
        writer.finish()?;

        assert_eq!(
            zip_media_route(&archive, Some(&archive.join("game.pce")))?,
            ZipMediaRoute::Generic
        );
        assert_eq!(
            zip_media_route(&archive, Some(&archive.join("disc/disc.cue")))?,
            ZipMediaRoute::PceCd
        );
        assert_eq!(
            zip_media_route(&archive, None)?,
            ZipMediaRoute::SelectionRequired
        );
        assert_eq!(
            super::super::super::list_rom_entries_in_zip(&archive)?
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>(),
            ["game.pce", "disc/disc.cue"]
        );
        let (loaded_path, loaded) = super::super::super::extract_rom_entry_path_from_zip(
            &archive,
            &archive.join("game.pce"),
        )?;
        assert_eq!(loaded_path, archive.join("game.pce"));
        assert_eq!(loaded, vec![0; 64]);
        Ok(())
    }
}

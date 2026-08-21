use crate::emu_backend::{ActiveSystem, ROM_EXTENSIONS};
use crate::rom_archive::ArchiveRomEntry;
use anyhow::Context;
use std::path::{Path, PathBuf};

pub(crate) fn extract_rom_from_zip(zip_path: &Path) -> anyhow::Result<(PathBuf, Vec<u8>)> {
    let file = std::fs::File::open(zip_path).context("Failed to open ZIP")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read ZIP archive")?;
    extract_rom_entries(&mut archive, zip_path)
}

pub(crate) fn list_rom_entries_in_zip(zip_path: &Path) -> anyhow::Result<Vec<ArchiveRomEntry>> {
    let file = std::fs::File::open(zip_path).context("Failed to open ZIP")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read ZIP archive")?;
    Ok(collect_rom_entries(&mut archive))
}

pub(crate) fn extract_rom_entry_from_zip(
    zip_path: &Path,
    entry_index: usize,
) -> anyhow::Result<(PathBuf, Vec<u8>)> {
    let file = std::fs::File::open(zip_path).context("Failed to open ZIP")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read ZIP archive")?;
    extract_rom_entry_by_index(&mut archive, zip_path, entry_index)
}

pub(crate) fn extract_rom_entry_path_from_zip(
    zip_path: &Path,
    virtual_rom_path: &Path,
) -> anyhow::Result<(PathBuf, Vec<u8>)> {
    let relative = virtual_rom_path.strip_prefix(zip_path).with_context(|| {
        format!(
            "Loaded ROM path '{}' is not inside archive '{}'",
            virtual_rom_path.display(),
            zip_path.display()
        )
    })?;
    let entry_name = zip_entry_name_from_relative_path(relative);
    let file = std::fs::File::open(zip_path).context("Failed to open ZIP")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read ZIP archive")?;
    let index = (0..archive.len())
        .find(|&i| {
            archive
                .by_index(i)
                .is_ok_and(|entry| entry.name() == entry_name)
        })
        .with_context(|| format!("Archive entry '{entry_name}' no longer exists"))?;
    extract_rom_entry_by_index(&mut archive, zip_path, index)
}

#[allow(dead_code)] // Used on WASM for drag-and-drop ROM loading
pub(crate) fn extract_rom_from_zip_bytes(
    data: &[u8],
    source_name: &str,
) -> anyhow::Result<(PathBuf, Vec<u8>)> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).context("Failed to read ZIP archive")?;
    let virtual_root = PathBuf::from(source_name);
    extract_rom_entries(&mut archive, &virtual_root)
}

fn extract_rom_entries<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    base_path: &Path,
) -> anyhow::Result<(PathBuf, Vec<u8>)> {
    let all_names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            Some(entry.name().to_string())
        })
        .collect();

    let rom_entries = collect_rom_entries(archive);

    match rom_entries.len() {
        0 => {
            let found_exts: Vec<String> = all_names
                .iter()
                .filter_map(|n| {
                    Path::new(n)
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| format!(".{}", e.to_ascii_lowercase()))
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let found_str = if found_exts.is_empty() {
                "archive is empty".to_string()
            } else {
                format!("found: {}", found_exts.join(", "))
            };
            anyhow::bail!(
                "No ROM files found in ZIP. Supported: .{}. ({found_str})",
                ROM_EXTENSIONS.join(", ."),
            )
        }
        1 => extract_rom_entry_by_index(archive, base_path, rom_entries[0].index),
        n => anyhow::bail!(
            "ZIP contains {n} ROM files; expected exactly 1. Found: {}",
            rom_entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn collect_rom_entries<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Vec<ArchiveRomEntry> {
    (0..archive.len())
        .filter_map(|index| {
            let entry = archive.by_index(index).ok()?;
            if entry.is_dir() {
                return None;
            }
            let name = entry.name().to_string();
            let system = ActiveSystem::from_path(Path::new(&name))?;
            Some(ArchiveRomEntry {
                index,
                name,
                system,
                uncompressed_size: entry.size(),
            })
        })
        .collect()
}

fn extract_rom_entry_by_index<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    base_path: &Path,
    entry_index: usize,
) -> anyhow::Result<(PathBuf, Vec<u8>)> {
    let mut entry = archive
        .by_index(entry_index)
        .with_context(|| format!("Failed to read entry #{entry_index} from ZIP"))?;
    let name = entry.name().to_string();
    if ActiveSystem::from_path(Path::new(&name)).is_none() {
        anyhow::bail!("Archive entry '{name}' is not a supported ROM");
    }
    let mut data = Vec::with_capacity(entry.size() as usize);
    std::io::Read::read_to_end(&mut entry, &mut data)
        .with_context(|| format!("Failed to decompress '{name}'"))?;
    let virtual_path = base_path.join(name);
    Ok((virtual_path, data))
}

fn zip_entry_name_from_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Clone)]
pub(crate) struct SlotInfo {
    pub labels: [String; 10],
    pub occupied: [bool; 10],
}

pub(crate) fn build_slot_info(rom_hash: Option<[u8; 32]>, system: ActiveSystem) -> SlotInfo {
    let mut labels: [String; 10] = Default::default();
    let mut occupied = [false; 10];
    for i in 0..10 {
        let slot = i as u8;
        let Some(hash) = rom_hash else {
            labels[i] = format!("Slot {slot}  (empty)");
            continue;
        };
        let Ok(path) = crate::save_paths::slot_path(
            system.storage_subdir(),
            system.state_extension(),
            hash,
            slot,
        ) else {
            labels[i] = format!("Slot {slot}  (empty)");
            continue;
        };

        if crate::platform::save_data_exists(&path) {
            occupied[i] = true;
            if let Some(stamp) = std::fs::metadata(&path)
                .ok()
                .and_then(|meta| crate::platform::format_file_modified_time(&meta))
            {
                labels[i] = format!("Slot {slot}  ({stamp})");
            } else {
                labels[i] = format!("Slot {slot}");
            }
        } else {
            labels[i] = format!("Slot {slot}  (empty)");
        }
    }
    SlotInfo { labels, occupied }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn zip_with_files(files: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        for (name, bytes) in files {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .expect("zip entry should start");
            writer.write_all(bytes).expect("zip entry should write");
        }
        writer.finish().expect("zip should finish").into_inner()
    }

    fn zip_with_file(name: &str, bytes: &[u8]) -> Vec<u8> {
        zip_with_files(&[(name, bytes)])
    }

    #[test]
    fn extracts_gba_rom_from_zip() {
        let rom = [0x12, 0x34, 0x56];
        let zip = zip_with_file("folder/game.gba", &rom);

        let (path, data) = extract_rom_from_zip_bytes(&zip, "archive.zip")
            .expect("GBA ROM inside ZIP should be supported");

        assert_eq!(path, PathBuf::from("archive.zip").join("folder/game.gba"));
        assert_eq!(data, rom);
    }

    #[test]
    fn extracts_pce_rom_from_zip() {
        let rom = [0x12, 0x34, 0x56];
        let zip = zip_with_file("folder/game.pce", &rom);

        let (path, data) = extract_rom_from_zip_bytes(&zip, "archive.zip")
            .expect("PCE ROM inside ZIP should be supported");

        assert_eq!(path, PathBuf::from("archive.zip").join("folder/game.pce"));
        assert_eq!(data, rom);
    }

    #[test]
    fn missing_rom_error_mentions_gba() {
        let zip = zip_with_file("readme.txt", b"not a rom");

        let err = extract_rom_from_zip_bytes(&zip, "archive.zip")
            .expect_err("ZIP without a supported ROM should fail")
            .to_string();

        assert!(err.contains(".gba"), "error was: {err}");
    }

    #[test]
    fn lists_multiple_roms_in_zip() {
        let zip = zip_with_files(&[
            ("folder/one.gb", &[1, 2, 3]),
            ("folder/two.gba", &[4, 5, 6, 7]),
            ("readme.txt", b"ignored"),
        ]);
        let cursor = Cursor::new(zip);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip should open");

        let entries = collect_rom_entries(&mut archive);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "folder/one.gb");
        assert_eq!(entries[0].system, ActiveSystem::GameBoy);
        assert_eq!(entries[1].name, "folder/two.gba");
        assert_eq!(entries[1].system, ActiveSystem::GameBoyAdvance);
        assert_eq!(entries[1].uncompressed_size, 4);
    }

    #[test]
    fn extracts_selected_rom_from_multi_rom_zip() {
        let zip = zip_with_files(&[
            ("folder/one.gb", &[1, 2, 3]),
            ("folder/two.gba", &[4, 5, 6, 7]),
        ]);
        let cursor = Cursor::new(zip);
        let mut archive = zip::ZipArchive::new(cursor).expect("zip should open");

        let (path, data) = extract_rom_entry_by_index(&mut archive, Path::new("archive.zip"), 1)
            .expect("selected ROM should extract");

        assert_eq!(path, PathBuf::from("archive.zip").join("folder/two.gba"));
        assert_eq!(data, [4, 5, 6, 7]);
    }
}

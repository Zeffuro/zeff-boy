use crate::emu_backend::ActiveSystem;
use anyhow::Context;
use std::path::{Path, PathBuf};

const ROM_EXTENSIONS: &[&str] = &["gb", "gbc", "sgb", "nes"];

pub(crate) fn extract_rom_from_zip(zip_path: &Path) -> anyhow::Result<(PathBuf, Vec<u8>)> {
    let file = std::fs::File::open(zip_path).context("Failed to open ZIP")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read ZIP archive")?;
    extract_rom_entries(&mut archive, zip_path)
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

    let rom_entries: Vec<(usize, String)> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();
            let ext = Path::new(&name).extension()?.to_str()?.to_ascii_lowercase();
            if ROM_EXTENSIONS.contains(&ext.as_str()) {
                Some((i, name))
            } else {
                None
            }
        })
        .collect();

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
        1 => {
            let (idx, name) = &rom_entries[0];
            let mut entry = archive
                .by_index(*idx)
                .with_context(|| format!("Failed to read '{name}' from ZIP"))?;
            let mut data = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut data)
                .with_context(|| format!("Failed to decompress '{name}'"))?;
            let virtual_path = base_path.join(name);
            Ok((virtual_path, data))
        }
        n => anyhow::bail!(
            "ZIP contains {n} ROM files; expected exactly 1. Found: {}",
            rom_entries
                .iter()
                .map(|(_, n)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
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
        let (subdir, ext) = match system {
            ActiveSystem::Nes => ("nes", "nstate"),
            ActiveSystem::GameBoy => ("gbc", "gbstate"),
        };
        let Ok(path) = crate::save_paths::slot_path(subdir, ext, hash, slot) else {
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

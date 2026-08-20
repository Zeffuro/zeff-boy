use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::debug::DebugWindowState;
use crate::debug::types::{
    FirmwareInventoryRow, FirmwareInventoryScanResult, FirmwareInventoryStatusKind,
};
use crate::settings::Settings;

use super::{
    CURRENT_FIRMWARE_IDS, draw_gb_boot_mode, draw_gba_boot_mode, draw_inventory,
    draw_sega_boot_mode,
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    poll_scan(state);
    let import_completed = poll_import(state);

    let busy = state.firmware_inventory.scan_receiver.is_some()
        || state.firmware_inventory.import_receiver.is_some();
    if ui
        .add_enabled(!busy, egui::Button::new("Import firmware..."))
        .clicked()
        && let Some(path) = crate::platform::FileDialog::new()
            .add_filter("Firmware", &["bin", "rom", "bios", "sms", "gg"])
            .set_title("Import firmware")
            .pick_file()
    {
        begin_import(path, ui.ctx().clone(), state);
    }
    ui.label(
        egui::RichText::new(format!(
            "Recognized imports: {}",
            crate::platform::managed_firmware_dir().display()
        ))
        .weak()
        .small(),
    );

    let mut start_scan = import_completed;
    ui.horizontal(|ui| {
        ui.label("Additional folder");
        if ui
            .text_edit_singleline(&mut settings.emulation.firmware_directory)
            .on_hover_text("Dedicated BIOS/firmware/system folders scan one subfolder level.")
            .changed()
        {
            state.firmware_inventory.needs_refresh = true;
            state.firmware_inventory.inventory = None;
        }

        if ui.button("Browse...").clicked() {
            let mut dialog = crate::platform::FileDialog::new().set_title("Select firmware folder");
            if let Some(current) = settings.emulation.firmware_directory_path() {
                dialog = dialog.set_directory(current);
            }
            if let Some(path) = dialog.pick_folder() {
                settings.emulation.firmware_directory = path.to_string_lossy().to_string();
                state.firmware_inventory.needs_refresh = true;
                state.firmware_inventory.inventory = None;
            }
        }

        if ui.add_enabled(!busy, egui::Button::new("Scan")).clicked() {
            start_scan = true;
        }
    });

    ui.separator();
    ui.strong("Game Boy / Game Boy Color");
    draw_gb_boot_mode(ui, settings);
    ui.separator();
    ui.strong("Game Boy Advance");
    draw_gba_boot_mode(ui, settings);
    ui.separator();
    ui.strong("Master System / Game Gear");
    draw_sega_boot_mode(ui, settings);

    let configured = settings.emulation.firmware_directory_path();
    if start_scan && state.firmware_inventory.scan_receiver.is_none() {
        begin_scan(
            configured.clone(),
            settings.emulation.firmware_search_dirs(),
            ui.ctx().clone(),
            state,
        );
    }

    if state.firmware_inventory.import_receiver.is_some()
        || state.firmware_inventory.scan_receiver.is_some()
    {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(if state.firmware_inventory.import_receiver.is_some() {
                "Importing firmware..."
            } else {
                "Scanning recognized firmware filenames..."
            });
        });
    }

    let showing_current_directory =
        !state.firmware_inventory.needs_refresh && state.firmware_inventory.directory == configured;
    if state.firmware_inventory.needs_refresh || !showing_current_directory {
        ui.label(
            egui::RichText::new("Scan to update firmware status.")
                .weak()
                .small(),
        );
    }
    if showing_current_directory && let Some(key) = draw_inventory(ui, state, !busy) {
        match crate::platform::remove_managed_firmware(&key) {
            Ok(()) => {
                state.firmware_inventory.needs_refresh = true;
                state.firmware_inventory.inventory = None;
                begin_scan(
                    configured,
                    settings.emulation.firmware_search_dirs(),
                    ui.ctx().clone(),
                    state,
                );
            }
            Err(error) => state.firmware_inventory.error = Some(error.to_string()),
        }
    }

    ui.separator();
    ui.label("FDS uses a recognized external BIOS when required.");
}

fn begin_import(path: PathBuf, context: egui::Context, state: &mut DebugWindowState) {
    let (sender, receiver) = std::sync::mpsc::channel();
    state.firmware_inventory.import_receiver = Some(receiver);
    state.firmware_inventory.error = None;

    let spawn = std::thread::Builder::new()
        .name("firmware-import".to_owned())
        .spawn(move || {
            let result =
                crate::platform::import_firmware_file(&path).map_err(|err| err.to_string());
            let _ = sender.send(result);
            context.request_repaint();
        });
    if let Err(err) = spawn {
        state.firmware_inventory.import_receiver = None;
        state.firmware_inventory.error = Some(format!("Failed to start firmware import: {err}"));
    }
}

fn poll_import(state: &mut DebugWindowState) -> bool {
    let result = match state.firmware_inventory.import_receiver.as_ref() {
        Some(receiver) => match receiver.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(
                "Firmware import stopped before producing a result.".to_owned(),
            )),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
        },
        None => None,
    };
    let Some(result) = result else {
        return false;
    };
    state.firmware_inventory.import_receiver = None;
    state.firmware_inventory.pending_removal = None;

    match result {
        Ok(imported) => {
            log::info!(
                "Imported native firmware {} ({}) to {}",
                imported.spec_id,
                imported.variant_id,
                imported.destination.display()
            );
            state.firmware_inventory.needs_refresh = true;
            state.firmware_inventory.inventory = None;
            state.firmware_inventory.error = None;
            true
        }
        Err(error) => {
            state.firmware_inventory.error = Some(error);
            false
        }
    }
}

fn begin_scan(
    configured_directory: Option<PathBuf>,
    roots: Vec<PathBuf>,
    context: egui::Context,
    state: &mut DebugWindowState,
) {
    let (sender, receiver) = std::sync::mpsc::channel();
    state.firmware_inventory.scan_receiver = Some(receiver);
    state.firmware_inventory.error = None;

    let spawn = std::thread::Builder::new()
        .name("firmware-scan".to_owned())
        .spawn(move || {
            let result = scan_firmware_roots(&roots);
            let _ = sender.send(FirmwareInventoryScanResult {
                configured_directory,
                result,
            });
            context.request_repaint();
        });
    if let Err(err) = spawn {
        state.firmware_inventory.scan_receiver = None;
        state.firmware_inventory.error = Some(format!("Failed to start firmware scan: {err}"));
    }
}

fn poll_scan(state: &mut DebugWindowState) {
    let result = match state.firmware_inventory.scan_receiver.as_ref() {
        Some(receiver) => match receiver.try_recv() {
            Ok(result) => Some(Ok(result)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(
                "Firmware scan stopped before producing a result.".to_owned(),
            )),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
        },
        None => None,
    };
    let Some(result) = result else {
        return;
    };
    state.firmware_inventory.scan_receiver = None;
    state.firmware_inventory.pending_removal = None;

    match result {
        Ok(FirmwareInventoryScanResult {
            configured_directory,
            result,
        }) => {
            state.firmware_inventory.directory = configured_directory;
            state.firmware_inventory.needs_refresh = false;
            match result {
                Ok((rows, inventory)) => {
                    state.firmware_inventory.rows = rows;
                    state.firmware_inventory.inventory = Some(inventory);
                    state.firmware_inventory.error = None;
                }
                Err(err) => {
                    state.firmware_inventory.rows.clear();
                    state.firmware_inventory.inventory = None;
                    state.firmware_inventory.error = Some(err);
                }
            }
        }
        Err(err) => state.firmware_inventory.error = Some(err),
    }
}

#[cfg(test)]
fn scan_firmware_rows(
    root: &Path,
) -> Result<
    (
        Vec<FirmwareInventoryRow>,
        std::sync::Arc<zeff_firmware::FirmwareInventory>,
    ),
    String,
> {
    if !root.is_dir() {
        return Err("Directory does not exist or is not accessible.".to_owned());
    }
    scan_firmware_roots(&[root.to_path_buf()])
}

fn scan_firmware_roots(
    roots: &[PathBuf],
) -> Result<
    (
        Vec<FirmwareInventoryRow>,
        std::sync::Arc<zeff_firmware::FirmwareInventory>,
    ),
    String,
> {
    let catalog = zeff_firmware::catalog_specs();
    let specs = CURRENT_FIRMWARE_IDS
        .iter()
        .filter_map(|id| catalog.iter().find(|spec| spec.id == *id))
        .collect::<Vec<_>>();
    let mut found = BTreeMap::<&str, Vec<FirmwareInventoryRow>>::new();
    let mut inventory = zeff_firmware::FirmwareInventory::new();

    let display_root = (roots.len() == 1).then(|| roots[0].as_path());
    for directory in crate::emu_backend::firmware::configured_firmware_inventory_dirs(roots) {
        if !directory.is_dir() {
            continue;
        }
        let mut entries = std::fs::read_dir(&directory)
            .map_err(|err| format!("Failed to read {}: {err}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("Failed to read {}: {err}", directory.display()))?;
        entries.sort_by_key(std::fs::DirEntry::path);

        for entry in entries {
            let file_type = entry
                .file_type()
                .map_err(|err| format!("Failed to inspect {}: {err}", entry.path().display()))?;
            if !file_type.is_file() {
                continue;
            }
            let Some(filename) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(spec) = specs.iter().copied().find(|spec| {
                spec.variants
                    .iter()
                    .any(|variant| variant.filename_matches(&filename))
            }) else {
                continue;
            };

            let bytes = std::fs::read(entry.path())
                .map_err(|err| format!("Failed to read {}: {err}", entry.path().display()))?;
            let inventory_entry =
                zeff_firmware::FirmwareInventoryEntry::from_bytes(bytes, Some(filename), catalog);
            inventory.add(inventory_entry.clone());
            found.entry(spec.id).or_default().push(row_for_entry(
                display_root.unwrap_or(Path::new("")),
                &entry.path(),
                spec,
                &inventory_entry,
            ));
        }
    }

    let mut rows = Vec::new();
    for spec in specs {
        if let Some(mut candidates) = found.remove(spec.id) {
            candidates.sort_by(|left, right| {
                let left_depth = left
                    .path
                    .as_deref()
                    .map_or(0, |path| Path::new(path).components().count());
                let right_depth = right
                    .path
                    .as_deref()
                    .map_or(0, |path| Path::new(path).components().count());
                left_depth
                    .cmp(&right_depth)
                    .then_with(|| left.path.cmp(&right.path))
            });
            rows.extend(candidates);
        } else {
            rows.push(FirmwareInventoryRow {
                firmware_id: spec.id.to_owned(),
                system: spec.system.to_owned(),
                firmware: spec.display_name.to_owned(),
                path: None,
                status: FirmwareInventoryStatusKind::NotFound,
                detail: "No matching filename found.".to_owned(),
                sha256_prefix: None,
                managed_key: None,
            });
        }
    }
    Ok((rows, std::sync::Arc::new(inventory)))
}

fn row_for_entry(
    root: &Path,
    path: &Path,
    spec: &zeff_firmware::FirmwareSpec,
    entry: &zeff_firmware::FirmwareInventoryEntry,
) -> FirmwareInventoryRow {
    let (status, detail) = match &entry.validation {
        zeff_firmware::ValidationStatus::KnownGood { variant_id, .. } => (
            FirmwareInventoryStatusKind::Recognized,
            format!("Recognized catalog variant {variant_id}."),
        ),
        zeff_firmware::ValidationStatus::UnknownHash {
            plausible_variant_ids,
            ..
        } => (
            FirmwareInventoryStatusKind::UnknownHash,
            format!(
                "Size and filename match, but the hash differs from: {}.",
                plausible_variant_ids.join(", ")
            ),
        ),
        zeff_firmware::ValidationStatus::WrongSize { expected, actual } => (
            FirmwareInventoryStatusKind::WrongSize,
            format!("{actual} bytes; expected {}.", expected.join(" or ")),
        ),
        zeff_firmware::ValidationStatus::NoMatchingSpec => (
            FirmwareInventoryStatusKind::UnknownHash,
            "Filename is recognized, but the file does not match the catalog.".to_owned(),
        ),
    };
    let display_path = path.strip_prefix(root).unwrap_or(path);
    FirmwareInventoryRow {
        firmware_id: spec.id.to_owned(),
        system: spec.system.to_owned(),
        firmware: spec.display_name.to_owned(),
        path: Some(display_path.to_string_lossy().to_string()),
        status,
        detail,
        sha256_prefix: Some(format!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            entry.digests.sha256[0],
            entry.digests.sha256[1],
            entry.digests.sha256[2],
            entry.digests.sha256[3],
            entry.digests.sha256[4],
            entry.digests.sha256[5]
        )),
        managed_key: managed_key_for_path(path, &crate::platform::managed_firmware_dir()),
    }
}

fn managed_key_for_path(path: &Path, managed_root: &Path) -> Option<String> {
    path.parent()
        .filter(|parent| *parent == managed_root)
        .and_then(|_| path.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "zeff_firmware_settings_{name}_{}",
            std::process::id()
        ))
    }

    #[test]
    fn scan_reads_only_catalog_filenames_and_reports_missing_systems() {
        let dir = test_dir("filtered");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("unrelated.bin"), vec![0; 1_000_000]).unwrap();
        std::fs::write(dir.join("gba_bios.bin"), vec![0; 16_384]).unwrap();

        let (rows, inventory) = scan_firmware_rows(&dir).unwrap();

        assert!(
            !rows
                .iter()
                .any(|row| row.path.as_deref() == Some("unrelated.bin"))
        );
        assert!(rows.iter().any(|row| {
            row.firmware_id == "nintendo.gba.bios"
                && row.status == FirmwareInventoryStatusKind::UnknownHash
                && row.managed_key.is_none()
        }));
        assert_eq!(inventory.entries().len(), 1);
        assert!(rows.iter().any(|row| {
            row.firmware_id == "nintendo.fds.bios"
                && row.status == FirmwareInventoryStatusKind::NotFound
        }));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn scan_keeps_subfolder_paths_distinct() {
        let root = test_dir("paths").join("BIOS");
        let child = root.join("SkyEmu");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(root.join("cgb_boot.bin"), vec![0; 2_304]).unwrap();
        std::fs::write(child.join("cgb_boot.bin"), vec![1; 2_304]).unwrap();

        let (rows, inventory) = scan_firmware_rows(&root).unwrap();
        let paths = rows
            .iter()
            .filter(|row| row.firmware_id == "nintendo.gb.boot.cgb")
            .filter_map(|row| row.path.as_deref().map(PathBuf::from))
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            [
                PathBuf::from("cgb_boot.bin"),
                PathBuf::from("SkyEmu").join("cgb_boot.bin"),
            ]
        );
        assert_eq!(inventory.entries().len(), 2);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn only_direct_managed_store_files_receive_removal_keys() {
        let managed = PathBuf::from("F:/AppData/zeff-boy/firmware");
        assert_eq!(
            managed_key_for_path(&managed.join("gba_bios.bin"), &managed).as_deref(),
            Some("gba_bios.bin")
        );
        assert_eq!(
            managed_key_for_path(&managed.join("nested/gba_bios.bin"), &managed),
            None
        );
        assert_eq!(
            managed_key_for_path(Path::new("F:/Configured/gba_bios.bin"), &managed),
            None
        );
    }
}

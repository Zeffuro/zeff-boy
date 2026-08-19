#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

use crate::debug::DebugWindowState;
#[cfg(not(target_arch = "wasm32"))]
use crate::debug::types::{
    FirmwareInventoryRow, FirmwareInventoryScanResult, FirmwareInventoryStatusKind,
};
use crate::settings::Settings;

#[cfg(not(target_arch = "wasm32"))]
const CURRENT_FIRMWARE_IDS: &[&str] = &[
    "nintendo.gb.boot.dmg",
    "nintendo.gb.boot.cgb",
    "nintendo.gba.bios",
    "nintendo.fds.bios",
    "sega.sms.boot",
    "sega.gg.boot",
];

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    ui.heading("Firmware");

    #[cfg(not(target_arch = "wasm32"))]
    draw_native(ui, settings, state);

    #[cfg(target_arch = "wasm32")]
    ui.label(
        egui::RichText::new("Browser firmware import is not available yet.")
            .weak()
            .small(),
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_native(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    poll_scan(state);

    let mut start_scan = false;
    ui.horizontal(|ui| {
        ui.label("Folder");
        if ui
            .text_edit_singleline(&mut settings.emulation.firmware_directory)
            .on_hover_text("Dedicated BIOS/firmware/system folders scan one subfolder level.")
            .changed()
        {
            state.firmware_inventory.needs_refresh = true;
        }

        if ui.button("Browse...").clicked() {
            let mut dialog = crate::platform::FileDialog::new().set_title("Select firmware folder");
            if let Some(current) = settings.emulation.firmware_directory_path() {
                dialog = dialog.set_directory(current);
            }
            if let Some(path) = dialog.pick_folder() {
                settings.emulation.firmware_directory = path.to_string_lossy().to_string();
                state.firmware_inventory.needs_refresh = true;
            }
        }

        let scanning = state.firmware_inventory.scan_receiver.is_some();
        if ui
            .add_enabled(!scanning, egui::Button::new("Scan"))
            .clicked()
        {
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

    let configured = settings.emulation.firmware_directory.trim();
    if configured.is_empty() {
        ui.label(egui::RichText::new("Choose a firmware folder to scan.").weak());
        return;
    }

    let path = Path::new(configured);
    if start_scan {
        begin_scan(path.to_path_buf(), ui.ctx().clone(), state);
    }

    if state.firmware_inventory.scan_receiver.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Scanning recognized firmware filenames...");
        });
    }

    let showing_current_directory = state.firmware_inventory.directory.as_deref() == Some(path);
    if state.firmware_inventory.needs_refresh || !showing_current_directory {
        ui.label(
            egui::RichText::new("Scan to update firmware status.")
                .weak()
                .small(),
        );
    }
    if showing_current_directory {
        draw_inventory(ui, state);
    }

    ui.separator();
    ui.label("FDS uses a recognized external BIOS when required.");
}

#[cfg(not(target_arch = "wasm32"))]
fn begin_scan(path: PathBuf, context: egui::Context, state: &mut DebugWindowState) {
    let (sender, receiver) = std::sync::mpsc::channel();
    state.firmware_inventory.scan_receiver = Some(receiver);
    state.firmware_inventory.error = None;

    let spawn = std::thread::Builder::new()
        .name("firmware-scan".to_owned())
        .spawn(move || {
            let result = scan_firmware_rows(&path);
            let _ = sender.send(FirmwareInventoryScanResult {
                directory: path,
                result,
            });
            context.request_repaint();
        });
    if let Err(err) = spawn {
        state.firmware_inventory.scan_receiver = None;
        state.firmware_inventory.error = Some(format!("Failed to start firmware scan: {err}"));
    }
}

#[cfg(not(target_arch = "wasm32"))]
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

    match result {
        Ok(FirmwareInventoryScanResult { directory, result }) => {
            state.firmware_inventory.directory = Some(directory);
            state.firmware_inventory.needs_refresh = false;
            match result {
                Ok(rows) => {
                    state.firmware_inventory.rows = rows;
                    state.firmware_inventory.error = None;
                }
                Err(err) => {
                    state.firmware_inventory.rows.clear();
                    state.firmware_inventory.error = Some(err);
                }
            }
        }
        Err(err) => {
            state.firmware_inventory.error = Some(err);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn scan_firmware_rows(root: &Path) -> Result<Vec<FirmwareInventoryRow>, String> {
    if !root.is_dir() {
        return Err("Directory does not exist or is not accessible.".to_owned());
    }

    let catalog = zeff_firmware::catalog_specs();
    let specs = CURRENT_FIRMWARE_IDS
        .iter()
        .filter_map(|id| catalog.iter().find(|spec| spec.id == *id))
        .collect::<Vec<_>>();
    let mut found = BTreeMap::<&str, Vec<FirmwareInventoryRow>>::new();

    for directory in
        crate::emu_backend::firmware::configured_firmware_inventory_dirs(&[root.to_path_buf()])
    {
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
            found.entry(spec.id).or_default().push(row_for_entry(
                root,
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
            });
        }
    }
    Ok(rows)
}

#[cfg(not(target_arch = "wasm32"))]
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
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_gba_boot_mode(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal(|ui| {
        ui.label("Boot mode");
        ui.selectable_value(
            &mut settings.emulation.gba_bios_mode,
            crate::settings::GbaBiosMode::Hle,
            "HLE",
        )
        .on_hover_text("Skip the boot animation and use built-in BIOS services.");
        ui.selectable_value(
            &mut settings.emulation.gba_bios_mode,
            crate::settings::GbaBiosMode::External,
            "External BIOS",
        )
        .on_hover_text("Run a recognized gba_bios.bin from reset.");
    });
    ui.label(
        egui::RichText::new("Applies when a ROM is next loaded.")
            .weak()
            .small(),
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_gb_boot_mode(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal(|ui| {
        ui.label("Startup");
        ui.selectable_value(
            &mut settings.emulation.gb_boot_rom_mode,
            crate::settings::GbBootRomMode::Skip,
            "Skip boot ROM",
        );
        ui.selectable_value(
            &mut settings.emulation.gb_boot_rom_mode,
            crate::settings::GbBootRomMode::External,
            "External boot ROM",
        );
    });
    ui.label(
        egui::RichText::new("Uses the recognized DMG or CGB boot ROM for the selected hardware mode on the next ROM load.")
            .weak()
            .small(),
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_sega_boot_mode(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal(|ui| {
        ui.label("Startup");
        ui.selectable_value(
            &mut settings.emulation.sega_boot_rom_mode,
            crate::settings::SegaBootRomMode::Skip,
            "Skip boot ROM",
        );
        ui.selectable_value(
            &mut settings.emulation.sega_boot_rom_mode,
            crate::settings::SegaBootRomMode::External,
            "External boot ROM",
        );
    });
    ui.label(
        egui::RichText::new(
            "Uses recognized regional SMS firmware or bios.gg on the next ROM load.",
        )
        .weak()
        .small(),
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_inventory(ui: &mut egui::Ui, state: &DebugWindowState) {
    if let Some(err) = &state.firmware_inventory.error {
        ui.label(egui::RichText::new(err).color(egui::Color32::YELLOW));
        return;
    }
    if state.firmware_inventory.rows.is_empty() {
        return;
    }

    let mut previous_system = None::<&str>;
    for row in &state.firmware_inventory.rows {
        if previous_system != Some(row.system.as_str()) {
            ui.add_space(8.0);
            ui.strong(&row.system);
            previous_system = Some(&row.system);
        }
        egui::Grid::new(("firmware_row", &row.firmware_id, row.path.as_deref()))
            .num_columns(4)
            .show(ui, |ui| {
                ui.label(&row.firmware);
                let (label, color) = match row.status {
                    FirmwareInventoryStatusKind::Recognized => {
                        ("Recognized", egui::Color32::LIGHT_GREEN)
                    }
                    FirmwareInventoryStatusKind::UnknownHash => {
                        ("Unknown hash", egui::Color32::YELLOW)
                    }
                    FirmwareInventoryStatusKind::WrongSize => ("Wrong size", egui::Color32::YELLOW),
                    FirmwareInventoryStatusKind::NotFound => ("Not found", egui::Color32::GRAY),
                };
                ui.label(egui::RichText::new(label).color(color))
                    .on_hover_text(&row.detail);
                ui.label(row.path.as_deref().unwrap_or("-"));
                ui.monospace(row.sha256_prefix.as_deref().unwrap_or("-"));
                ui.end_row();
            });
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
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

        let rows = scan_firmware_rows(&dir).unwrap();

        assert!(
            !rows
                .iter()
                .any(|row| row.path.as_deref() == Some("unrelated.bin"))
        );
        assert!(rows.iter().any(|row| {
            row.firmware_id == "nintendo.gba.bios"
                && row.status == FirmwareInventoryStatusKind::UnknownHash
        }));
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

        let rows = scan_firmware_rows(&root).unwrap();
        let paths = rows
            .iter()
            .filter(|row| row.firmware_id == "nintendo.gb.boot.cgb")
            .filter_map(|row| row.path.as_deref())
            .collect::<Vec<_>>();

        assert_eq!(paths, ["cgb_boot.bin", "SkyEmu\\cgb_boot.bin"]);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }
}

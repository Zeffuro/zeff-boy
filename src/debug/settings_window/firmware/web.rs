use crate::debug::DebugWindowState;
use crate::debug::types::{FirmwareInventoryRow, FirmwareInventoryStatusKind};
use crate::settings::Settings;

use super::{
    CURRENT_FIRMWARE_IDS, draw_gb_boot_mode, draw_gba_boot_mode, draw_inventory,
    draw_sega_boot_mode,
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    poll_operation(state);
    if state.firmware_inventory.needs_refresh {
        refresh_inventory(state);
    }
    let pending_file = state.firmware_inventory.pending_file.borrow_mut().take();
    if let Some((name, bytes)) = pending_file {
        match crate::platform::import_firmware(
            name,
            bytes,
            state.firmware_inventory.web_operation_result.clone(),
        ) {
            Ok(imported) => {
                state.firmware_inventory.web_operation_pending = true;
                state.firmware_inventory.error = None;
                log::info!(
                    "Persisting browser firmware {} ({})",
                    imported.spec_id,
                    imported.variant_id
                );
            }
            Err(error) => state.firmware_inventory.error = Some(error.to_string()),
        }
    }

    if ui
        .add_enabled(
            !state.firmware_inventory.web_operation_pending,
            egui::Button::new("Import firmware..."),
        )
        .clicked()
    {
        crate::platform::FileDialog::new()
            .add_filter("Firmware", &["bin", "rom", "bios"])
            .set_title("Import firmware")
            .pick_file_web(state.firmware_inventory.pending_file.clone());
    }
    ui.label(
        egui::RichText::new("Recognized firmware is stored in this browser.")
            .weak()
            .small(),
    );

    ui.separator();
    ui.strong("Game Boy / Game Boy Color");
    draw_gb_boot_mode(ui, settings);
    ui.separator();
    ui.strong("Game Boy Advance");
    draw_gba_boot_mode(ui, settings);
    ui.separator();
    ui.strong("Master System / Game Gear");
    draw_sega_boot_mode(ui, settings);
    let removal_enabled = !state.firmware_inventory.web_operation_pending;
    if let Some(key) = draw_inventory(ui, state, removal_enabled) {
        match crate::platform::remove_firmware(
            &key,
            state.firmware_inventory.web_operation_result.clone(),
        ) {
            Ok(()) => {
                state.firmware_inventory.web_operation_pending = true;
                state.firmware_inventory.error = None;
            }
            Err(error) => state.firmware_inventory.error = Some(error.to_string()),
        }
    }
    if state.firmware_inventory.web_operation_pending {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Saving firmware changes...");
        });
    }
}

fn poll_operation(state: &mut DebugWindowState) {
    let result = state
        .firmware_inventory
        .web_operation_result
        .borrow_mut()
        .take();
    let Some(result) = result else {
        return;
    };
    state.firmware_inventory.web_operation_pending = false;
    state.firmware_inventory.pending_removal = None;
    match result {
        Ok(()) => {
            state.firmware_inventory.error = None;
            refresh_inventory(state);
        }
        Err(error) => state.firmware_inventory.error = Some(error),
    }
}

fn refresh_inventory(state: &mut DebugWindowState) {
    let inventory = crate::platform::firmware_inventory_snapshot();
    state.firmware_inventory.rows = rows_for_inventory(&inventory);
    state.firmware_inventory.inventory = Some(inventory);
    state.firmware_inventory.directory = None;
    state.firmware_inventory.needs_refresh = false;
    state.firmware_inventory.pending_removal = None;
}

fn rows_for_inventory(inventory: &zeff_firmware::FirmwareInventory) -> Vec<FirmwareInventoryRow> {
    let catalog = zeff_firmware::catalog_specs();
    let mut rows = Vec::new();
    for spec in CURRENT_FIRMWARE_IDS
        .iter()
        .filter_map(|id| catalog.iter().find(|spec| spec.id == *id))
    {
        let mut found = inventory
            .entries()
            .iter()
            .filter(|entry| {
                matches!(
                    &entry.validation,
                    zeff_firmware::ValidationStatus::KnownGood { spec_id, .. }
                        if spec_id == spec.id
                )
            })
            .map(|entry| row_for_imported_entry(spec, entry))
            .collect::<Vec<_>>();
        if found.is_empty() {
            rows.push(FirmwareInventoryRow {
                firmware_id: spec.id.to_owned(),
                system: spec.system.to_owned(),
                firmware: spec.display_name.to_owned(),
                path: None,
                status: FirmwareInventoryStatusKind::NotFound,
                detail: "No recognized firmware imported.".to_owned(),
                sha256_prefix: None,
                managed_key: None,
            });
        } else {
            found.sort_by(|left, right| left.path.cmp(&right.path));
            rows.extend(found);
        }
    }
    rows
}

fn row_for_imported_entry(
    spec: &zeff_firmware::FirmwareSpec,
    entry: &zeff_firmware::FirmwareInventoryEntry,
) -> FirmwareInventoryRow {
    let variant_id = match &entry.validation {
        zeff_firmware::ValidationStatus::KnownGood { variant_id, .. } => variant_id,
        _ => unreachable!("browser store retains only recognized firmware"),
    };
    FirmwareInventoryRow {
        firmware_id: spec.id.to_owned(),
        system: spec.system.to_owned(),
        firmware: spec.display_name.to_owned(),
        path: entry.original_filename.clone(),
        status: FirmwareInventoryStatusKind::Recognized,
        detail: format!("Recognized catalog variant {variant_id}."),
        sha256_prefix: Some(format!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            entry.digests.sha256[0],
            entry.digests.sha256[1],
            entry.digests.sha256[2],
            entry.digests.sha256[3],
            entry.digests.sha256[4],
            entry.digests.sha256[5]
        )),
        managed_key: Some(crate::platform::firmware_storage_key(&entry.bytes)),
    }
}

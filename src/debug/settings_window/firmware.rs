#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use crate::debug::DebugWindowState;
use crate::debug::types::FirmwareInventoryStatusKind;
use crate::settings::Settings;

pub(super) const CURRENT_FIRMWARE_IDS: &[&str] = &[
    "nintendo.gb.boot.dmg",
    "nintendo.gb.boot.cgb",
    "nintendo.gba.bios",
    "nintendo.fds.bios",
    "coleco.vision.bios",
    "sega.sms.boot",
    "sega.gg.boot",
];

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    #[cfg(not(target_arch = "wasm32"))]
    native::draw(ui, settings, state);

    #[cfg(target_arch = "wasm32")]
    web::draw(ui, settings, state);
}

pub(super) fn draw_gba_boot_mode(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Boot mode");
        ui.selectable_value(
            &mut settings.emulation.gba_bios_mode,
            crate::settings::GbaBiosMode::Hle,
            "HLE",
        );
        ui.selectable_value(
            &mut settings.emulation.gba_bios_mode,
            crate::settings::GbaBiosMode::External,
            "External BIOS",
        );
    });
    ui.label(
        egui::RichText::new("Applies when a ROM is next loaded.")
            .weak()
            .small(),
    );
}

pub(super) fn draw_gb_boot_mode(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal_wrapped(|ui| {
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
        egui::RichText::new("Applies on next ROM load.")
            .weak()
            .small(),
    );
}

pub(super) fn draw_sega_boot_mode(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal_wrapped(|ui| {
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
        egui::RichText::new("Applies on next ROM load.")
            .weak()
            .small(),
    );
}

pub(super) fn draw_inventory(
    ui: &mut egui::Ui,
    state: &mut DebugWindowState,
    removal_enabled: bool,
) -> Option<String> {
    if let Some(err) = &state.firmware_inventory.error {
        let (_, color) = status_label(ui, FirmwareInventoryStatusKind::UnknownHash);
        ui.label(egui::RichText::new(err).color(color));
        return None;
    }
    if state.firmware_inventory.rows.is_empty() {
        return None;
    }

    let recognized = state
        .firmware_inventory
        .rows
        .iter()
        .filter(|row| row.status == FirmwareInventoryStatusKind::Recognized)
        .count();
    let issues = state
        .firmware_inventory
        .rows
        .iter()
        .filter(|row| {
            matches!(
                row.status,
                FirmwareInventoryStatusKind::UnknownHash | FirmwareInventoryStatusKind::WrongSize
            )
        })
        .count();
    let missing = state
        .firmware_inventory
        .rows
        .iter()
        .filter(|row| row.status == FirmwareInventoryStatusKind::NotFound)
        .count();
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.strong("Scan results");
        let (_, recognized_color) = status_label(ui, FirmwareInventoryStatusKind::Recognized);
        ui.label(egui::RichText::new(format!("{recognized} recognized")).color(recognized_color));
        if issues > 0 {
            let (_, issue_color) = status_label(ui, FirmwareInventoryStatusKind::UnknownHash);
            ui.label(egui::RichText::new(format!("{issues} need attention")).color(issue_color));
        }
        if missing > 0 {
            ui.label(egui::RichText::new(format!("{missing} not found")).weak());
        }
    });

    let mut removal = None;
    let mut pending_removal = state.firmware_inventory.pending_removal.clone();
    let mut previous_system = None::<&str>;
    let mut start = 0;
    while start < state.firmware_inventory.rows.len() {
        let first = &state.firmware_inventory.rows[start];
        let end = start
            + state.firmware_inventory.rows[start..]
                .iter()
                .take_while(|row| row.firmware_id == first.firmware_id)
                .count();
        let candidates = &state.firmware_inventory.rows[start..end];
        if previous_system != Some(first.system.as_str()) {
            ui.add_space(10.0);
            ui.strong(&first.system);
            previous_system = Some(&first.system);
        }
        let files = candidates.iter().filter(|row| row.path.is_some()).count();
        let recognized = candidates
            .iter()
            .filter(|row| row.status == FirmwareInventoryStatusKind::Recognized)
            .count();
        let issues = candidates
            .iter()
            .filter(|row| {
                matches!(
                    row.status,
                    FirmwareInventoryStatusKind::UnknownHash
                        | FirmwareInventoryStatusKind::WrongSize
                )
            })
            .count();
        ui.horizontal_wrapped(|ui| {
            ui.label(&first.firmware);
            if files == 0 {
                let (label, color) = status_label(ui, FirmwareInventoryStatusKind::NotFound);
                ui.label(egui::RichText::new(label).color(color));
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "{files} {}",
                        if files == 1 { "file" } else { "files" }
                    ))
                    .weak(),
                );
                if recognized > 0 {
                    let (_, color) = status_label(ui, FirmwareInventoryStatusKind::Recognized);
                    ui.label(egui::RichText::new(format!("{recognized} recognized")).color(color));
                }
                if issues > 0 {
                    let (_, color) = status_label(ui, FirmwareInventoryStatusKind::UnknownHash);
                    ui.label(egui::RichText::new(format!("{issues} need attention")).color(color));
                }
            }
        });

        if files > 0 {
            egui::CollapsingHeader::new(format!("File details ({files})"))
                .id_salt(("firmware_details", &first.firmware_id))
                .show(ui, |ui| {
                    for (index, row) in candidates.iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        let (label, color) = status_label(ui, row.status);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new(label).color(color))
                                .on_hover_text(&row.detail);
                            if let Some(hash) = row.sha256_prefix.as_deref() {
                                ui.monospace(format!("{hash}…"));
                            }
                        });
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(row.path.as_deref().unwrap_or("Unavailable"))
                                    .monospace(),
                            )
                            .wrap(),
                        );
                        ui.add(egui::Label::new(&row.detail).wrap());
                        if let Some(key) = row.managed_key.as_deref() {
                            ui.horizontal_wrapped(|ui| {
                                if pending_removal.as_deref() == Some(key) {
                                    ui.label(
                                        egui::RichText::new("Remove this imported file?").weak(),
                                    );
                                    if ui
                                        .add_enabled(removal_enabled, egui::Button::new("Remove"))
                                        .clicked()
                                    {
                                        removal = Some(key.to_owned());
                                        pending_removal = None;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        pending_removal = None;
                                    }
                                } else if ui
                                    .add_enabled(removal_enabled, egui::Button::new("Remove"))
                                    .on_hover_text("Remove this app-managed firmware file.")
                                    .clicked()
                                {
                                    pending_removal = Some(key.to_owned());
                                }
                            });
                        }
                    }
                });
        }
        start = end;
    }
    state.firmware_inventory.pending_removal = pending_removal;
    removal
}

fn status_label(
    ui: &egui::Ui,
    status: FirmwareInventoryStatusKind,
) -> (&'static str, egui::Color32) {
    let dark = ui.visuals().dark_mode;
    match status {
        FirmwareInventoryStatusKind::Recognized => (
            "Recognized",
            if dark {
                egui::Color32::from_rgb(120, 210, 155)
            } else {
                egui::Color32::from_rgb(0, 105, 60)
            },
        ),
        FirmwareInventoryStatusKind::UnknownHash => (
            "Unknown hash",
            if dark {
                egui::Color32::from_rgb(245, 200, 95)
            } else {
                egui::Color32::from_rgb(145, 90, 0)
            },
        ),
        FirmwareInventoryStatusKind::WrongSize => (
            "Wrong size",
            if dark {
                egui::Color32::from_rgb(245, 200, 95)
            } else {
                egui::Color32::from_rgb(145, 90, 0)
            },
        ),
        FirmwareInventoryStatusKind::NotFound => ("Not found", ui.visuals().weak_text_color()),
    }
}

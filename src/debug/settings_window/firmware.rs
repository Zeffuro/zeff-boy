#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use super::{
    layout::row,
    search::{self, SettingId as Id},
};
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
    row(ui, Id::FirmwareGbaBiosBootMode, Some("Boot mode"), |ui| {
        ui.horizontal(|ui| {
            let hle = ui.selectable_value(
                &mut settings.emulation.gba_bios_mode,
                crate::settings::GbaBiosMode::Hle,
                "HLE",
            );
            ui.selectable_value(
                &mut settings.emulation.gba_bios_mode,
                crate::settings::GbaBiosMode::External,
                "External BIOS",
            );
            hle
        })
        .inner
    });
    ui.label(
        egui::RichText::new("Applies when a ROM is next loaded.")
            .weak()
            .small(),
    );
}

pub(super) fn draw_gb_boot_mode(ui: &mut egui::Ui, settings: &mut Settings) {
    row(
        ui,
        Id::FirmwareGameBoyBootRomStartup,
        Some("Startup"),
        |ui| {
            ui.horizontal(|ui| {
                let skip = ui.selectable_value(
                    &mut settings.emulation.gb_boot_rom_mode,
                    crate::settings::GbBootRomMode::Skip,
                    "Skip boot ROM",
                );
                ui.selectable_value(
                    &mut settings.emulation.gb_boot_rom_mode,
                    crate::settings::GbBootRomMode::External,
                    "External boot ROM",
                );
                skip
            })
            .inner
        },
    );
    ui.label(
        egui::RichText::new("Applies on next ROM load.")
            .weak()
            .small(),
    );
}

pub(super) fn draw_sega_boot_mode(ui: &mut egui::Ui, settings: &mut Settings) {
    row(ui, Id::FirmwareSegaBootRomStartup, Some("Startup"), |ui| {
        ui.horizontal(|ui| {
            let skip = ui.selectable_value(
                &mut settings.emulation.sega_boot_rom_mode,
                crate::settings::SegaBootRomMode::Skip,
                "Skip boot ROM",
            );
            ui.selectable_value(
                &mut settings.emulation.sega_boot_rom_mode,
                crate::settings::SegaBootRomMode::External,
                "External boot ROM",
            );
            skip
        })
        .inner
    });
    ui.label(
        egui::RichText::new("Applies on next ROM load.")
            .weak()
            .small(),
    );
}

pub(super) fn draw_inventory(
    ui: &mut egui::Ui,
    settings: &Settings,
    state: &mut DebugWindowState,
    removal_enabled: bool,
) -> Option<String> {
    let details_requested = search::requested(ui, Id::FirmwareFirmwareFileDetails);
    let removal_requested = search::requested(ui, Id::FirmwareRemoveImportedFirmware);
    if let Some(err) = &state.firmware_inventory.error {
        search::conditional(
            ui,
            Id::FirmwareFirmwareFileDetails,
            Id::FirmwareImportFirmware,
            false,
            "Import firmware or resolve the inventory error before viewing file details.",
        );
        search::conditional(
            ui,
            Id::FirmwareRemoveImportedFirmware,
            Id::FirmwareImportFirmware,
            false,
            "Only app-managed imported firmware can be removed.",
        );
        let (_, color) = status_label(ui, FirmwareInventoryStatusKind::UnknownHash);
        ui.label(egui::RichText::new(err).color(color));
        return None;
    }
    if state.firmware_inventory.rows.is_empty() {
        search::conditional(
            ui,
            Id::FirmwareFirmwareFileDetails,
            Id::FirmwareImportFirmware,
            false,
            "Import firmware before viewing file details.",
        );
        search::conditional(
            ui,
            Id::FirmwareRemoveImportedFirmware,
            Id::FirmwareImportFirmware,
            false,
            "Only app-managed imported firmware can be removed.",
        );
        return None;
    }
    let details_available = state
        .firmware_inventory
        .rows
        .iter()
        .any(|row| row.path.is_some());
    search::conditional(
        ui,
        Id::FirmwareFirmwareFileDetails,
        Id::FirmwareImportFirmware,
        details_available,
        "Import firmware before viewing file details.",
    );
    let removal_target = removal_requested
        .then(|| {
            state
                .firmware_inventory
                .rows
                .iter()
                .find_map(|row| row.managed_key.clone())
        })
        .flatten();
    search::conditional(
        ui,
        Id::FirmwareRemoveImportedFirmware,
        Id::FirmwareImportFirmware,
        removal_target.is_some(),
        "Only app-managed imported firmware can be removed.",
    );

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
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.strong("File inventory");
        let (_, recognized_color) = status_label(ui, FirmwareInventoryStatusKind::Recognized);
        ui.label(
            egui::RichText::new(format!(
                "{recognized} recognized {}",
                if recognized == 1 { "file" } else { "files" }
            ))
            .color(recognized_color),
        );
        if issues > 0 {
            let (_, issue_color) = status_label(ui, FirmwareInventoryStatusKind::UnknownHash);
            ui.label(egui::RichText::new(format!("{issues} need attention")).color(issue_color));
        }
    });

    draw_configured_readiness(ui, settings, &state.firmware_inventory.rows);

    let mut removal = None;
    let mut pending_removal = state.firmware_inventory.pending_removal.clone();
    let mut details_targeted = false;
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
            let system_end = start
                + state.firmware_inventory.rows[start..]
                    .iter()
                    .take_while(|row| row.system == first.system)
                    .count();
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                ui.strong(&first.system);
                draw_system_readiness(
                    ui,
                    settings,
                    &state.firmware_inventory.rows[start..system_end],
                );
            });
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
            if recognized > 0 {
                let (_, color) = status_label(ui, FirmwareInventoryStatusKind::Recognized);
                let label = if issues > 0 {
                    "Available · attention"
                } else {
                    "Available"
                };
                ui.label(egui::RichText::new(label).color(color));
            } else if issues > 0 {
                let (_, color) = status_label(ui, FirmwareInventoryStatusKind::UnknownHash);
                ui.label(egui::RichText::new("Needs attention").color(color));
            } else {
                let (label, color) = identity_status(ui, settings, &first.firmware_id);
                ui.label(egui::RichText::new(label).color(color));
            }
            if files > 0 {
                ui.label(
                    egui::RichText::new(format!(
                        "{files} {}",
                        if files == 1 { "file" } else { "files" }
                    ))
                    .weak(),
                );
            }
        });

        if files > 0 {
            let target_details = details_requested && !details_targeted;
            details_targeted |= target_details;
            let target_removal = removal_target.as_deref().is_some_and(|key| {
                candidates
                    .iter()
                    .any(|row| row.managed_key.as_deref() == Some(key))
            });
            let details = egui::CollapsingHeader::new(format!("File details ({files})"))
                .id_salt(("firmware_details", &first.firmware_id))
                .open((target_details || target_removal).then_some(true))
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
                                    let response = ui
                                        .add_enabled(removal_enabled, egui::Button::new("Remove"));
                                    if removal_target.as_deref() == Some(key) {
                                        search::target(
                                            ui,
                                            Id::FirmwareRemoveImportedFirmware,
                                            &response,
                                        );
                                    }
                                    if response.clicked() {
                                        removal = Some(key.to_owned());
                                        pending_removal = None;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        pending_removal = None;
                                    }
                                } else {
                                    let response = ui
                                        .add_enabled(removal_enabled, egui::Button::new("Remove"))
                                        .on_hover_text("Remove this app-managed firmware file.");
                                    if removal_target.as_deref() == Some(key) {
                                        search::target(
                                            ui,
                                            Id::FirmwareRemoveImportedFirmware,
                                            &response,
                                        );
                                    }
                                    if response.clicked() {
                                        pending_removal = Some(key.to_owned());
                                    }
                                }
                            });
                        }
                    }
                });
            if target_details {
                search::target(
                    ui,
                    Id::FirmwareFirmwareFileDetails,
                    &details.header_response,
                );
            }
        }
        start = end;
    }
    state.firmware_inventory.pending_removal = pending_removal;
    removal
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FirmwareRequirement {
    Optional,
    Required,
    ContentDependent,
}

fn requirement(settings: &Settings, firmware_id: &str) -> FirmwareRequirement {
    match firmware_id {
        "nintendo.gba.bios"
            if settings.emulation.gba_bios_mode == crate::settings::GbaBiosMode::External =>
        {
            FirmwareRequirement::Required
        }
        "nintendo.gb.boot.dmg" | "nintendo.gb.boot.cgb"
            if settings.emulation.gb_boot_rom_mode == crate::settings::GbBootRomMode::External =>
        {
            FirmwareRequirement::ContentDependent
        }
        "sega.sms.boot" | "sega.gg.boot"
            if settings.emulation.sega_boot_rom_mode
                == crate::settings::SegaBootRomMode::External =>
        {
            FirmwareRequirement::ContentDependent
        }
        "nintendo.fds.bios" | "coleco.vision.bios" => FirmwareRequirement::ContentDependent,
        _ => FirmwareRequirement::Optional,
    }
}

fn has_usable(rows: &[crate::debug::types::FirmwareInventoryRow], firmware_id: &str) -> bool {
    rows.iter().any(|row| {
        row.firmware_id == firmware_id && row.status == FirmwareInventoryStatusKind::Recognized
    })
}

fn draw_configured_readiness(
    ui: &mut egui::Ui,
    settings: &Settings,
    rows: &[crate::debug::types::FirmwareInventoryRow],
) {
    ui.add_space(6.0);
    ui.strong("Configured-mode readiness");

    if requirement(settings, "nintendo.gba.bios") == FirmwareRequirement::Required
        && !has_usable(rows, "nintendo.gba.bios")
    {
        let (_, color) = status_label(ui, FirmwareInventoryStatusKind::UnknownHash);
        ui.label(
            egui::RichText::new(
                "Game Boy Advance needs a usable BIOS for the selected firmware mode.",
            )
            .color(color),
        );
    } else {
        let (_, color) = status_label(ui, FirmwareInventoryStatusKind::Recognized);
        ui.label(
            egui::RichText::new("No selected firmware mode has a known unmet requirement.")
                .color(color),
        );
    }

    if rows.iter().any(|row| {
        requirement(settings, &row.firmware_id) == FirmwareRequirement::ContentDependent
            && !has_usable(rows, &row.firmware_id)
    }) {
        ui.label(
            egui::RichText::new(
                "Some requirements depend on the system and hardware detected when content is loaded.",
            )
            .weak()
            .small(),
        );
    }
}

fn draw_system_readiness(
    ui: &mut egui::Ui,
    settings: &Settings,
    rows: &[crate::debug::types::FirmwareInventoryRow],
) {
    let unmet = rows.iter().any(|row| {
        requirement(settings, &row.firmware_id) == FirmwareRequirement::Required
            && !has_usable(rows, &row.firmware_id)
    });
    let unresolved = rows.iter().any(|row| {
        requirement(settings, &row.firmware_id) == FirmwareRequirement::ContentDependent
            && !has_usable(rows, &row.firmware_id)
    });
    if unmet {
        let (_, color) = status_label(ui, FirmwareInventoryStatusKind::UnknownHash);
        ui.label(egui::RichText::new("Needs firmware for configured mode").color(color));
    } else if unresolved {
        ui.label(egui::RichText::new("Depends on loaded content").weak());
    } else {
        let (_, color) = status_label(ui, FirmwareInventoryStatusKind::Recognized);
        ui.label(egui::RichText::new("Ready for configured mode").color(color));
    }
}

fn identity_status(
    ui: &egui::Ui,
    settings: &Settings,
    firmware_id: &str,
) -> (&'static str, egui::Color32) {
    match requirement(settings, firmware_id) {
        FirmwareRequirement::Required => {
            let (_, color) = status_label(ui, FirmwareInventoryStatusKind::UnknownHash);
            ("Not ready for current mode", color)
        }
        FirmwareRequirement::ContentDependent => (
            "Not found · required for matching content",
            ui.visuals().weak_text_color(),
        ),
        FirmwareRequirement::Optional => (
            "Not found · optional in current mode",
            ui.visuals().weak_text_color(),
        ),
    }
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

#[cfg(test)]
mod tests {
    use super::{FirmwareRequirement, requirement};
    use crate::settings::{GbBootRomMode, GbaBiosMode, SegaBootRomMode, Settings};

    #[test]
    fn built_in_modes_leave_external_images_optional() {
        let settings = Settings::default();

        assert!(matches!(
            requirement(&settings, "nintendo.gba.bios"),
            FirmwareRequirement::Optional
        ));
        assert!(matches!(
            requirement(&settings, "nintendo.gb.boot.dmg"),
            FirmwareRequirement::Optional
        ));
        assert!(matches!(
            requirement(&settings, "sega.sms.boot"),
            FirmwareRequirement::Optional
        ));
    }

    #[test]
    fn external_modes_reflect_when_hardware_selection_is_still_unknown() {
        let mut settings = Settings::default();
        settings.emulation.gba_bios_mode = GbaBiosMode::External;
        settings.emulation.gb_boot_rom_mode = GbBootRomMode::External;
        settings.emulation.sega_boot_rom_mode = SegaBootRomMode::External;

        assert!(matches!(
            requirement(&settings, "nintendo.gba.bios"),
            FirmwareRequirement::Required
        ));
        for firmware_id in [
            "nintendo.gb.boot.dmg",
            "nintendo.gb.boot.cgb",
            "sega.sms.boot",
            "sega.gg.boot",
        ] {
            assert!(matches!(
                requirement(&settings, firmware_id),
                FirmwareRequirement::ContentDependent
            ));
        }
    }

    #[test]
    fn content_specific_bios_requirements_are_conditional_without_content() {
        let settings = Settings::default();

        for firmware_id in ["nintendo.fds.bios", "coleco.vision.bios"] {
            assert!(matches!(
                requirement(&settings, firmware_id),
                FirmwareRequirement::ContentDependent
            ));
        }
    }
}

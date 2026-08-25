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
    "sega.sms.boot",
    "sega.gg.boot",
];

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    ui.heading("Firmware");

    #[cfg(not(target_arch = "wasm32"))]
    native::draw(ui, settings, state);

    #[cfg(target_arch = "wasm32")]
    web::draw(ui, settings, state);
}

pub(super) fn draw_gba_boot_mode(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.horizontal(|ui| {
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
        egui::RichText::new("Applies on next ROM load.")
            .weak()
            .small(),
    );
}

pub(super) fn draw_sega_boot_mode(ui: &mut egui::Ui, settings: &mut Settings) {
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
        ui.label(egui::RichText::new(err).color(egui::Color32::YELLOW));
        return None;
    }
    if state.firmware_inventory.rows.is_empty() {
        return None;
    }

    let mut previous_system = None::<&str>;
    let mut removal = None;
    let mut pending_removal = state.firmware_inventory.pending_removal.clone();
    for row in &state.firmware_inventory.rows {
        if previous_system != Some(row.system.as_str()) {
            ui.add_space(8.0);
            ui.strong(&row.system);
            previous_system = Some(&row.system);
        }
        egui::Grid::new(("firmware_row", &row.firmware_id, row.path.as_deref()))
            .num_columns(5)
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
                if let Some(key) = row.managed_key.as_deref() {
                    if pending_removal.as_deref() == Some(key) {
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(removal_enabled, egui::Button::new("Confirm"))
                                .clicked()
                            {
                                removal = Some(key.to_owned());
                                pending_removal = None;
                            }
                            if ui.button("Cancel").clicked() {
                                pending_removal = None;
                            }
                        });
                    } else if ui
                        .add_enabled(removal_enabled, egui::Button::new("Remove"))
                        .on_hover_text("Remove this app-managed firmware file.")
                        .clicked()
                    {
                        pending_removal = Some(key.to_owned());
                    }
                } else {
                    ui.label("");
                }
                ui.end_row();
            });
    }
    state.firmware_inventory.pending_removal = pending_removal;
    removal
}

use std::path::Path;

use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Firmware");
    ui.label(egui::RichText::new("Use BIOS/firmware files you provide.").weak());

    #[cfg(not(target_arch = "wasm32"))]
    draw_native_firmware_dir(ui, settings);

    #[cfg(target_arch = "wasm32")]
    ui.label(
        egui::RichText::new("Browser firmware import is not available yet.")
            .weak()
            .small(),
    );

    ui.separator();
    ui.label("Required firmware is checked when loading content that needs it.");
    ui.label(
        egui::RichText::new("Optional boot BIOS support is still explicit opt-in work.").weak(),
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_native_firmware_dir(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Folder");
        ui.text_edit_singleline(&mut settings.emulation.firmware_directory)
            .on_hover_text("Dedicated BIOS/firmware/system folders also scan one subfolder level.");

        if ui.button("Browse...").clicked() {
            let mut dialog = crate::platform::FileDialog::new().set_title("Select firmware folder");
            if let Some(current) = settings.emulation.firmware_directory_path() {
                dialog = dialog.set_directory(current);
            }
            if let Some(path) = dialog.pick_folder() {
                settings.emulation.firmware_directory = path.to_string_lossy().to_string();
            }
        }
    });

    let firmware_dir = settings.emulation.firmware_directory.trim();
    if firmware_dir.is_empty() {
        ui.label(
            egui::RichText::new("No firmware folder set.")
                .weak()
                .small(),
        );
    } else if !Path::new(firmware_dir).is_dir() {
        ui.label(
            egui::RichText::new("Directory does not exist or is not accessible.")
                .color(egui::Color32::YELLOW)
                .small(),
        );
    } else {
        ui.label(
            egui::RichText::new("Active firmware folder.")
                .weak()
                .small(),
        );
    }

    ui.label(
        egui::RichText::new("Recommended: a dedicated BIOS folder.")
            .weak()
            .small(),
    );
}

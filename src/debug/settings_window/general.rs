use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Startup & focus");
    ui.checkbox(
        &mut settings.emulation.pause_on_unfocus,
        "Pause when window loses focus",
    );
    #[cfg(not(target_arch = "wasm32"))]
    ui.checkbox(
        &mut settings.ui.check_for_updates,
        "Check for updates on startup",
    );

    ui.separator();
    ui.heading("Recent content");
    if settings.recent_roms.is_empty() {
        ui.label(egui::RichText::new("No recent content yet.").weak());
    } else {
        ui.label(format!(
            "{} recent item(s) are stored locally.",
            settings.recent_roms.len()
        ));
        if ui.button("Clear recent content").clicked() {
            settings.recent_roms.clear();
        }
    }
}

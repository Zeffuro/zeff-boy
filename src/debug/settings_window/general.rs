use crate::settings::Settings;

use super::{
    layout::{checkbox, helper, row},
    search::SettingId as Id,
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Startup & focus");
    checkbox(
        ui,
        Id::GeneralPauseWhenWindowLosesFocus,
        None,
        &mut settings.emulation.pause_on_unfocus,
    );
    #[cfg(not(target_arch = "wasm32"))]
    checkbox(
        ui,
        Id::GeneralCheckForUpdatesOnStartup,
        None,
        &mut settings.ui.check_for_updates,
    );

    ui.add_space(22.0);
    ui.heading("Recent content");
    if settings.recent_roms.is_empty() {
        helper(ui, egui::RichText::new("No recent content yet.").weak());
        row(ui, Id::GeneralClearRecentContent, None, |ui| {
            ui.add_enabled(false, egui::Button::new("Clear recent content"))
        })
        .on_hover_text("There is no recent content to clear.");
    } else {
        helper(
            ui,
            format!(
                "{} recent item(s) are stored locally.",
                settings.recent_roms.len()
            ),
        );
        if row(ui, Id::GeneralClearRecentContent, None, |ui| {
            ui.button("Clear recent content")
        })
        .clicked()
        {
            settings.recent_roms.clear();
        }
    }
}

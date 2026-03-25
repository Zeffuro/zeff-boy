mod audio;
mod controls;
mod display;
mod emulation;

use crate::debug::DebugWindowState;
use crate::settings::Settings;

pub(crate) fn draw_settings_window(
    ctx: &egui::Context,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    open: &mut bool,
) {
    egui::Window::new("Settings")
        .open(open)
        .default_width(400.0)
        .default_height(500.0)
        .resizable(true)
        .show(ctx, |ui| {
            const TABS: &[&str] = &["Emulation", "Controls", "Audio", "UI"];

            ui.horizontal(|ui| {
                for (i, &label) in TABS.iter().enumerate() {
                    if ui
                        .selectable_label(state.settings_tab == i, label)
                        .clicked()
                    {
                        state.settings_tab = i;
                    }
                }
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    match state.settings_tab {
                        0 => emulation::draw(ui, settings),
                        1 => controls::draw(ui, settings, state),
                        2 => audio::draw(ui, settings),
                        3 => display::draw(ui, settings),
                        _ => {}
                    }

                    ui.separator();
                    if ui.button("Reset to defaults").clicked() {
                        *settings = Settings::default();
                        state.rebinding_action = None;
                    }
                });
        });
}


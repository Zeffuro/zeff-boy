mod audio;
mod camera;
mod controls;
mod emulation;
mod ui;
mod video;

use crate::debug::DebugWindowState;
use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

pub(crate) struct SettingsContext<'a> {
    pub active_system: Option<ActiveSystem>,
    pub gb_hardware_mode_label: Option<&'a str>,
    pub is_pocket_camera: bool,
}

pub(crate) fn draw_settings_window(
    ctx: &egui::Context,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    open: &mut bool,
    constrain_rect: egui::Rect,
    emu: &SettingsContext<'_>,
) {
    let active_system = emu.active_system;
    let gb_hardware_mode_label = emu.gb_hardware_mode_label;
    let is_pocket_camera = emu.is_pocket_camera;
    egui::Window::new("Settings")
        .open(open)
        .default_width(400.0)
        .default_height(500.0)
        .resizable(true)
        .constrain_to(constrain_rect)
        .show(ctx, |ui| {
            const TABS: &[&str] = &["Emulation", "Controls", "Audio", "Video", "UI", "Camera"];

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
                        0 => emulation::draw(ui, settings, active_system),
                        1 => controls::draw(ui, settings, state, active_system),
                        2 => audio::draw(ui, settings),
                        3 => video::draw(
                            ui,
                            settings,
                            active_system,
                            gb_hardware_mode_label,
                            is_pocket_camera,
                        ),
                        4 => ui::draw(ui, settings),
                        5 => camera::draw(ui, settings, state),
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

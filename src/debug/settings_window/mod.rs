mod audio;
mod camera;
mod controls;
mod emulation;
mod firmware;
mod ui;
mod video;

use crate::debug::DebugWindowState;
use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

pub(crate) struct SettingsContext<'a> {
    pub active_system: Option<ActiveSystem>,
    pub gb_hardware_mode_label: Option<&'a str>,
    pub is_pocket_camera: bool,
    #[cfg(target_arch = "wasm32")]
    pub nes_palette_file_slot: crate::platform::FileDataSlot,
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn draw_settings_window(
    ctx: &egui::Context,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    open: &mut bool,
    constrain_rect: egui::Rect,
    emu: &SettingsContext<'_>,
) {
    egui::Window::new("Settings")
        .open(open)
        .default_width(400.0)
        .default_height(500.0)
        .resizable(true)
        .constrain_to(constrain_rect)
        .show(ctx, |ui| {
            draw_settings_content(ui, settings, state, emu);
        });
}

pub(crate) fn draw_settings_content(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    emu: &SettingsContext<'_>,
) {
    const TABS: &[&str] = &[
        "Emulation",
        "Firmware",
        "Controls",
        "Audio",
        "Video",
        "UI",
        "Camera",
    ];

    ui.horizontal_wrapped(|ui| {
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
                0 => emulation::draw(ui, settings, emu.active_system),
                1 => firmware::draw(ui, settings, state),
                2 => controls::draw(ui, settings, state, emu.active_system),
                3 => audio::draw(ui, settings),
                4 => video::draw(
                    ui,
                    settings,
                    emu.active_system,
                    emu.gb_hardware_mode_label,
                    emu.is_pocket_camera,
                    #[cfg(target_arch = "wasm32")]
                    emu.nes_palette_file_slot.clone(),
                ),
                5 => ui::draw(ui, settings),
                6 => camera::draw(ui, settings, state),
                _ => {}
            }

            ui.separator();
            if ui.button("Reset to defaults").clicked() {
                *settings = Settings::default();
                state.rebinding_action = None;
                state.rebinding_gamepad = None;
                state.rebinding_gamepad_p2 = None;
                state.rebinding_ws_gamepad = None;
                state.rebinding_gamepad_action = None;
            }
        });
}

pub(super) fn draw_console_section_header(
    ui: &mut egui::Ui,
    label: &str,
    active_system: Option<ActiveSystem>,
    target: ActiveSystem,
) {
    ui.horizontal(|ui| {
        ui.heading(label);
        if active_system == Some(target) {
            ui.label(egui::RichText::new("(active)").weak().italics().small());
        }
    });
}

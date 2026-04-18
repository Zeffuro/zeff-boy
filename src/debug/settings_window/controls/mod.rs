mod gamepad_actions;
mod joypad;
mod shortcuts;
pub(super) mod tilt;

use crate::debug::DebugWindowState;
use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

pub(super) fn draw(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    active_system: Option<ActiveSystem>,
) {
    ui.heading("Global");
    shortcuts::draw(ui, settings, state);
    gamepad_actions::draw(ui, settings, state);

    ui.separator();
    ui.heading("Shared Console Input");
    ui.label(
        egui::RichText::new("Applies across supported consoles.")
            .weak()
            .small(),
    );
    joypad::draw(ui, settings, state);

    ui.separator();
    super::draw_console_section_header(ui, "Game Boy", active_system, ActiveSystem::GameBoy);
    tilt::draw(ui, settings, state);

    ui.separator();
    super::draw_console_section_header(ui, "NES", active_system, ActiveSystem::Nes);
    ui.label(
        egui::RichText::new(
            "NES-specific input bindings can be added here as console features expand.",
        )
        .weak()
        .small(),
    );
}

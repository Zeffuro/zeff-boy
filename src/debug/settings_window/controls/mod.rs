mod gamepad_actions;
mod joypad;
mod shortcuts;
pub(super) mod tilt;

use crate::debug::DebugWindowState;
use crate::settings::Settings;

pub(super) fn draw(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
) {
    joypad::draw(ui, settings, state);
    shortcuts::draw(ui, settings, state);
    gamepad_actions::draw(ui, settings, state);
    tilt::draw(ui, settings, state);
}


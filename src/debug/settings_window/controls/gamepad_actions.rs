use crate::debug::DebugWindowState;
use crate::settings::{GamepadAction, Settings};

use super::super::{
    layout::row,
    search::{self, SettingId as Id},
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    let requested = ACTIONS
        .into_iter()
        .map(action_id)
        .any(|id| search::requested(ui, id));
    egui::CollapsingHeader::new("Gamepad Actions")
        .default_open(false)
        .open(requested.then_some(true))
        .show(ui, |ui| {
            if state.rebinding_gamepad_action.is_some() {
                ui.label(
                    egui::RichText::new("Press a gamepad button for action...")
                        .color(egui::Color32::YELLOW),
                );
            }
            for action in ACTIONS {
                let id = action_id(action);
                let bound = settings.gamepad_bindings.get_action(action);
                let display = if bound.is_empty() {
                    "(not bound)".to_string()
                } else {
                    bound.to_string()
                };
                if row(ui, id, None, |ui| {
                    let capture_label = if state.rebinding_gamepad_action == Some(action) {
                        format!("Press button... ({display})")
                    } else {
                        display
                    };
                    ui.button(capture_label)
                })
                .clicked()
                {
                    super::joypad::clear_capture(state);
                    state.rebinding_gamepad_action = Some(action);
                }
                if !settings.gamepad_bindings.get_action(action).is_empty()
                    && row(ui, id, Some(""), |ui| ui.small_button("✕")).clicked()
                {
                    super::joypad::clear_capture(state);
                    settings.gamepad_bindings.set_action(action, "");
                }
            }
        });
}

const ACTIONS: [GamepadAction; 4] = [
    GamepadAction::SpeedUp,
    GamepadAction::Rewind,
    GamepadAction::Pause,
    GamepadAction::Turbo,
];

pub(super) fn action_id(action: GamepadAction) -> Id {
    match action {
        GamepadAction::SpeedUp => Id::InputDevicesGamepadSpeedUpAction,
        GamepadAction::Rewind => Id::InputDevicesGamepadRewindAction,
        GamepadAction::Pause => Id::InputDevicesGamepadPauseAction,
        GamepadAction::Turbo => Id::InputDevicesGamepadTurboAction,
    }
}

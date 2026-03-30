use crate::debug::DebugWindowState;
use crate::settings::{GamepadAction, Settings};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    egui::CollapsingHeader::new("Gamepad Actions")
        .default_open(false)
        .show(ui, |ui| {
            if state.rebinding_gamepad_action.is_some() {
                ui.label(
                    egui::RichText::new("Press a gamepad button for action...")
                        .color(egui::Color32::YELLOW),
                );
            }
            egui::Grid::new("gamepad_action_bindings")
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for action in [
                        GamepadAction::SpeedUp,
                        GamepadAction::Rewind,
                        GamepadAction::Pause,
                        GamepadAction::Turbo,
                    ] {
                        ui.label(gamepad_action_label(action));
                        let bound = settings.gamepad_bindings.get_action(action);
                        let display = if bound.is_empty() {
                            "(not bound)".to_string()
                        } else {
                            bound.to_string()
                        };
                        let capture_label =
                            if state.rebinding_gamepad_action == Some(action) {
                                format!("Press button... ({display})")
                            } else {
                                display
                            };
                        if ui.button(capture_label).clicked() {
                            state.rebinding_gamepad_action = Some(action);
                            state.rebinding_gamepad = None;
                            state.rebinding_action = None;
                            state.rebinding_shortcut = None;
                            state.rebinding_speedup = false;
                            state.rebinding_rewind = false;
                        }
                        if !settings.gamepad_bindings.get_action(action).is_empty()
                            && ui.small_button("✕").clicked()
                        {
                            settings.gamepad_bindings.set_action(action, "");
                        }
                        ui.end_row();
                    }
                });
        });
}

fn gamepad_action_label(action: GamepadAction) -> &'static str {
    match action {
        GamepadAction::SpeedUp => "Speed-up (hold)",
        GamepadAction::Rewind => "Rewind (hold)",
        GamepadAction::Pause => "Pause (toggle)",
        GamepadAction::Turbo => "Turbo / rapid-fire (hold)",
    }
}


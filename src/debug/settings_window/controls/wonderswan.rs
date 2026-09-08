use super::{BindingSource, controller_diagram::DiagramAction, joypad};
use crate::debug::DebugWindowState;
use crate::settings::{InputBindingAction, Settings, WonderSwanButton};

pub(super) fn draw_focused(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    source: BindingSource,
) {
    if source == BindingSource::Controller {
        ui.label(egui::RichText::new("Additional direct mappings. The Player 1 D-pad already controls X horizontally and Y vertically. Unbound adds no extra mapping.").weak());
        ui.add_space(8.0);
    }
    egui::Grid::new("wonderswan_bindings")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Control");
            ui.strong(match source {
                BindingSource::Keyboard => "Keyboard key",
                BindingSource::Controller => "Controller button",
            });
            ui.end_row();
            for &action in WonderSwanButton::ALL {
                ui.label(action.label());
                let (capturing, label) = match source {
                    BindingSource::Keyboard => (
                        state.rebinding_action == Some(InputBindingAction::WonderSwan(action)),
                        joypad::key_label(settings.ws_key_bindings.get(action)),
                    ),
                    BindingSource::Controller => (
                        state.rebinding_ws_gamepad == Some(action),
                        joypad::gamepad_label(settings.gamepad_bindings.get_ws(action)),
                    ),
                };
                let label = if capturing {
                    "Press a control…".to_owned()
                } else {
                    label
                };
                let response = ui.add_sized([170.0, 26.0], egui::Button::new(label));
                if response.clicked() {
                    joypad::begin_capture(state, 1, source, DiagramAction::WonderSwan(action));
                }
                if source == BindingSource::Controller {
                    response.context_menu(|ui| {
                        if ui.button("Clear this mapping").clicked() {
                            let previous = settings.clone();
                            settings.gamepad_bindings.set_ws(action, "");
                            joypad::clear_capture(state);
                            state.settings_ui.undo = Some(previous);
                            state.settings_ui.undo_baseline = Some(settings.clone());
                            state.settings_ui.undo_label =
                                Some("Controller mapping cleared.".into());
                            ui.close();
                        }
                    });
                }
                ui.end_row();
            }
        });
    ui.add_space(8.0);
    if ui.button("Restore default mappings…").clicked() {
        state.settings_ui.player_reset_confirmation = Some(match source {
            BindingSource::Keyboard => super::super::PlayerResetTarget::WonderSwanKeyboard,
            BindingSource::Controller => super::super::PlayerResetTarget::WonderSwanGamepad,
        });
    }
    joypad::draw_player_reset_confirmation(ui, settings, state);
    if source == BindingSource::Controller {
        ui.menu_button("More options", |ui| {
            if ui.button("Clear all direct mappings…").clicked() {
                state.settings_ui.player_reset_confirmation =
                    Some(super::super::PlayerResetTarget::WonderSwanClear);
                ui.close();
            }
        });
    }
}

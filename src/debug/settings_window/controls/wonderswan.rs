use crate::debug::DebugWindowState;
use crate::settings::{InputBindingAction, Settings, WonderSwanButton};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    egui::CollapsingHeader::new("WonderSwan Controls")
        .default_open(true)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "WonderSwan has two four-way button diamonds. Horizontal games usually use \
                     the X diamond for movement; vertical games usually use the Y diamond. The \
                     generic gamepad d-pad follows display rotation automatically, while these \
                     direct bindings always target the named WS button.",
                )
                .weak()
                .small(),
            );

            egui::Grid::new("wonderswan_key_bindings")
                .num_columns(4)
                .spacing([12.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Button");
                    ui.strong("Keyboard");
                    ui.strong("Gamepad");
                    ui.strong("");
                    ui.end_row();

                    for &action in WonderSwanButton::ALL {
                        ui.label(action.label());

                        let key_name = format!("{:?}", settings.ws_key_bindings.get(action));
                        let label = if state.rebinding_action
                            == Some(InputBindingAction::WonderSwan(action))
                        {
                            format!("Press key... ({key_name})")
                        } else {
                            key_name
                        };
                        if ui.button(label).clicked() {
                            state.rebinding_action = Some(InputBindingAction::WonderSwan(action));
                            state.rebinding_gamepad = None;
                            state.rebinding_gamepad_p2 = None;
                            state.rebinding_ws_gamepad = None;
                            state.rebinding_shortcut = None;
                            state.rebinding_speedup = false;
                            state.rebinding_rewind = false;
                        }

                        let button_name = settings.gamepad_bindings.get_ws(action);
                        let display = if button_name.is_empty() {
                            "(not bound)".to_string()
                        } else {
                            button_name.to_string()
                        };
                        let gp_label = if state.rebinding_ws_gamepad == Some(action) {
                            format!("Press btn... ({display})")
                        } else {
                            display
                        };
                        if ui.button(gp_label).clicked() {
                            state.rebinding_ws_gamepad = Some(action);
                            state.rebinding_action = None;
                            state.rebinding_gamepad = None;
                            state.rebinding_gamepad_p2 = None;
                            state.rebinding_shortcut = None;
                            state.rebinding_speedup = false;
                            state.rebinding_rewind = false;
                        }
                        if !settings.gamepad_bindings.get_ws(action).is_empty()
                            && ui.small_button("Clear").clicked()
                        {
                            settings.gamepad_bindings.set_ws(action, "");
                            state.rebinding_ws_gamepad = None;
                        }

                        ui.end_row();
                    }
                });

            if ui.button("Reset WonderSwan keys to defaults").clicked() {
                settings.ws_key_bindings = crate::settings::WonderSwanKeyBindings::default();
                state.rebinding_action = None;
            }
            if ui.button("Reset WonderSwan gamepad to defaults").clicked() {
                settings.gamepad_bindings.reset_wonderswan_defaults();
                state.rebinding_ws_gamepad = None;
            }
            if ui
                .button("Clear direct WonderSwan gamepad bindings")
                .clicked()
            {
                settings.gamepad_bindings.clear_wonderswan_direct_bindings();
                state.rebinding_ws_gamepad = None;
            }
        });
}

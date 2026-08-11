use crate::debug::DebugWindowState;
use crate::settings::{BindingAction, InputBindingAction, Settings};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    egui::CollapsingHeader::new("Joypad Bindings")
        .default_open(true)
        .show(ui, |ui| {
            if let Some(action) = state.rebinding_action {
                let label = match action {
                    InputBindingAction::Joypad(a) => joypad_label_for_player(1, a),
                    InputBindingAction::JoypadP2(a) => joypad_label_for_player(2, a),
                    InputBindingAction::Tilt(a) => super::tilt::tilt_label(a).to_string(),
                    InputBindingAction::WonderSwan(a) => a.label().to_string(),
                };
                ui.label(
                    egui::RichText::new(format!("Press a key for {}...", label))
                        .color(egui::Color32::YELLOW),
                );
            }
            if state.rebinding_gamepad.is_some() {
                ui.label(
                    egui::RichText::new("Press a P1 gamepad button...")
                        .color(egui::Color32::YELLOW),
                );
            }
            if state.rebinding_gamepad_p2.is_some() {
                ui.label(
                    egui::RichText::new("Press a P2 gamepad button...")
                        .color(egui::Color32::YELLOW),
                );
            }
            egui::Grid::new("joypad_combined")
                .num_columns(5)
                .spacing([12.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Button");
                    ui.strong("Keyboard P1");
                    ui.strong("Gamepad P1");
                    ui.strong("Keyboard P2");
                    ui.strong("Gamepad P2");
                    ui.end_row();

                    for &action in BindingAction::ALL {
                        ui.label(joypad_label(action));

                        let key_name = format!("{:?}", settings.key_bindings.get(action));
                        let kb_label =
                            if state.rebinding_action == Some(InputBindingAction::Joypad(action)) {
                                format!("Press key... ({key_name})")
                            } else {
                                key_name
                            };
                        if ui.button(kb_label).clicked() {
                            state.rebinding_action = Some(InputBindingAction::Joypad(action));
                            state.rebinding_gamepad = None;
                            state.rebinding_gamepad_p2 = None;
                            state.rebinding_ws_gamepad = None;
                            state.rebinding_shortcut = None;
                            state.rebinding_speedup = false;
                            state.rebinding_rewind = false;
                        }

                        let button_name = settings.gamepad_bindings.get(action).to_owned();
                        let gp_label = if state.rebinding_gamepad == Some(action) {
                            format!("Press btn... ({button_name})")
                        } else {
                            button_name
                        };
                        if ui.button(gp_label).clicked() {
                            state.rebinding_gamepad = Some(action);
                            state.rebinding_gamepad_p2 = None;
                            state.rebinding_ws_gamepad = None;
                            state.rebinding_action = None;
                            state.rebinding_shortcut = None;
                            state.rebinding_speedup = false;
                            state.rebinding_rewind = false;
                        }

                        let key_name_p2 = format!("{:?}", settings.key_bindings_p2.get(action));
                        let kb_label_p2 = if state.rebinding_action
                            == Some(InputBindingAction::JoypadP2(action))
                        {
                            format!("Press key... ({key_name_p2})")
                        } else {
                            key_name_p2
                        };
                        if ui.button(kb_label_p2).clicked() {
                            state.rebinding_action = Some(InputBindingAction::JoypadP2(action));
                            state.rebinding_gamepad = None;
                            state.rebinding_gamepad_p2 = None;
                            state.rebinding_ws_gamepad = None;
                            state.rebinding_shortcut = None;
                            state.rebinding_speedup = false;
                            state.rebinding_rewind = false;
                        }

                        let button_name_p2 = settings.gamepad_bindings.get_p2(action).to_owned();
                        let gp_label_p2 = if state.rebinding_gamepad_p2 == Some(action) {
                            format!("Press btn... ({button_name_p2})")
                        } else {
                            button_name_p2
                        };
                        if ui.button(gp_label_p2).clicked() {
                            state.rebinding_gamepad_p2 = Some(action);
                            state.rebinding_gamepad = None;
                            state.rebinding_ws_gamepad = None;
                            state.rebinding_action = None;
                            state.rebinding_shortcut = None;
                            state.rebinding_speedup = false;
                            state.rebinding_rewind = false;
                        }

                        ui.end_row();
                    }
                });
            if ui.button("Reset gamepad to defaults").clicked() {
                settings.gamepad_bindings = crate::settings::GamepadBindings::default();
                state.rebinding_gamepad = None;
                state.rebinding_gamepad_p2 = None;
                state.rebinding_ws_gamepad = None;
                state.rebinding_gamepad_action = None;
            }
        });
}

fn joypad_label_for_player(player: u8, action: BindingAction) -> String {
    format!("P{player} {}", joypad_label(action))
}

fn joypad_label(action: BindingAction) -> &'static str {
    match action {
        BindingAction::Up => "Up",
        BindingAction::Down => "Down",
        BindingAction::Left => "Left",
        BindingAction::Right => "Right",
        BindingAction::A => "A",
        BindingAction::B => "B",
        BindingAction::L => "L",
        BindingAction::R => "R",
        BindingAction::Start => "Start",
        BindingAction::Select => "Select",
    }
}

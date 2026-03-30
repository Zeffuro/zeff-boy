use crate::debug::DebugWindowState;
use crate::settings::{BindingAction, InputBindingAction, Settings};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    egui::CollapsingHeader::new("Joypad Bindings")
        .default_open(true)
        .show(ui, |ui| {
            if let Some(action) = state.rebinding_action {
                let label = match action {
                    InputBindingAction::Joypad(a) => joypad_label(a),
                    InputBindingAction::Tilt(a) => super::tilt::tilt_label(a),
                };
                ui.label(
                    egui::RichText::new(format!("Press a key for {}...", label))
                        .color(egui::Color32::YELLOW),
                );
            }
            if state.rebinding_gamepad.is_some() {
                ui.label(
                    egui::RichText::new("Press a gamepad button...")
                        .color(egui::Color32::YELLOW),
                );
            }
            egui::Grid::new("joypad_combined")
                .num_columns(3)
                .spacing([12.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Button");
                    ui.strong("Keyboard");
                    ui.strong("Gamepad");
                    ui.end_row();

                    for action in [
                        BindingAction::Up,
                        BindingAction::Down,
                        BindingAction::Left,
                        BindingAction::Right,
                        BindingAction::A,
                        BindingAction::B,
                        BindingAction::Start,
                        BindingAction::Select,
                    ] {
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
                state.rebinding_gamepad_action = None;
            }
        });
}

fn joypad_label(action: BindingAction) -> &'static str {
    match action {
        BindingAction::Up => "Up",
        BindingAction::Down => "Down",
        BindingAction::Left => "Left",
        BindingAction::Right => "Right",
        BindingAction::A => "A",
        BindingAction::B => "B",
        BindingAction::Start => "Start",
        BindingAction::Select => "Select",
    }
}


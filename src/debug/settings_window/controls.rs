use crate::debug::DebugWindowState;
use crate::settings::{
    BindingAction, GamepadAction, InputBindingAction, LeftStickMode, Settings, ShortcutAction,
    TiltBindingAction, TiltInputMode,
};

pub(super) fn draw(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
) {
    egui::CollapsingHeader::new("Joypad Bindings")
        .default_open(true)
        .show(ui, |ui| {
            if let Some(action) = state.rebinding_action {
                let label = match action {
                    InputBindingAction::Joypad(a) => joypad_label(a),
                    InputBindingAction::Tilt(a) => tilt_label(a),
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

    egui::CollapsingHeader::new("Shortcuts")
        .default_open(true)
        .show(ui, |ui| {
            if state.rebinding_shortcut.is_some()
                || state.rebinding_speedup
                || state.rebinding_rewind
            {
                ui.label(
                    egui::RichText::new("Press a key to rebind...")
                        .color(egui::Color32::YELLOW),
                );
            }
            egui::Grid::new("shortcuts_grid")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    // Speed-up (hold)
                    ui.label("Speed-up (hold)");
                    let key_label = if state.rebinding_speedup {
                        format!("Press key... ({})", settings.speedup_key)
                    } else {
                        settings.speedup_key.clone()
                    };
                    if ui.button(key_label).clicked() {
                        state.rebinding_speedup = true;
                        state.rebinding_action = None;
                        state.rebinding_shortcut = None;
                        state.rebinding_rewind = false;
                    }
                    ui.end_row();

                    // Rewind (hold)
                    ui.label("Rewind (hold)");
                    let rewind_label = if state.rebinding_rewind {
                        format!("Press key... ({})", settings.rewind_key)
                    } else {
                        settings.rewind_key.clone()
                    };
                    if ui.button(rewind_label).clicked() {
                        state.rebinding_rewind = true;
                        state.rebinding_action = None;
                        state.rebinding_shortcut = None;
                        state.rebinding_speedup = false;
                    }
                    ui.end_row();

                    // All ShortcutAction bindings
                    for &action in ShortcutAction::ALL {
                        ui.label(action.label());
                        let key_str = settings.shortcut_bindings.key_str(action).to_owned();
                        let capture_label = if state.rebinding_shortcut == Some(action) {
                            format!("Press key... ({key_str})")
                        } else {
                            key_str
                        };
                        if ui.button(capture_label).clicked() {
                            state.rebinding_shortcut = Some(action);
                            state.rebinding_action = None;
                            state.rebinding_speedup = false;
                            state.rebinding_rewind = false;
                        }
                        ui.end_row();
                    }
                });
            ui.label(
                egui::RichText::new(
                    "Additional hardcoded bindings: Ctrl-R = Reset, \
                     Alt-Enter = Fullscreen, ` = Speed-up, \
                     LShift = Turbo (rapid-fire), 0-9 = select save slot",
                )
                .weak()
                .small(),
            );
        });

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

    egui::CollapsingHeader::new("MBC7 Tilt")
        .default_open(false)
        .show(ui, |ui| {
            egui::ComboBox::from_label("Left stick behavior")
                .selected_text(match settings.left_stick_mode {
                    LeftStickMode::Auto => "Auto (Tilt on MBC7, D-pad otherwise)",
                    LeftStickMode::Tilt => "Always Tilt",
                    LeftStickMode::Dpad => "Always D-pad",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.left_stick_mode,
                        LeftStickMode::Auto,
                        "Auto (Tilt on MBC7, D-pad otherwise)",
                    );
                    ui.selectable_value(
                        &mut settings.left_stick_mode,
                        LeftStickMode::Tilt,
                        "Always Tilt",
                    );
                    ui.selectable_value(
                        &mut settings.left_stick_mode,
                        LeftStickMode::Dpad,
                        "Always D-pad",
                    );
                });
            egui::ComboBox::from_label("Tilt input source")
                .selected_text(match settings.tilt_input_mode {
                    TiltInputMode::Keyboard => "Keyboard (WASD)",
                    TiltInputMode::Mouse => "Mouse",
                    TiltInputMode::Auto => "Auto-detect",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.tilt_input_mode,
                        TiltInputMode::Keyboard,
                        "Keyboard (WASD)",
                    );
                    ui.selectable_value(
                        &mut settings.tilt_input_mode,
                        TiltInputMode::Mouse,
                        "Mouse",
                    );
                    ui.selectable_value(
                        &mut settings.tilt_input_mode,
                        TiltInputMode::Auto,
                        "Auto-detect",
                    );
                });
            ui.checkbox(&mut settings.tilt_invert_x, "Invert tilt X");
            ui.checkbox(&mut settings.tilt_invert_y, "Invert tilt Y");
            ui.checkbox(
                &mut settings.stick_tilt_bypass_lerp,
                "Direct left-stick tilt (bypass lerp)",
            );
            ui.add(
                egui::Slider::new(&mut settings.tilt_sensitivity, 0.1..=3.0)
                    .text("Tilt sensitivity"),
            );
            ui.add(
                egui::Slider::new(&mut settings.tilt_lerp, 0.0..=1.0).text("Tilt smoothing"),
            );
            ui.add(
                egui::Slider::new(&mut settings.tilt_deadzone, 0.0..=0.5).text("Tilt deadzone"),
            );

            ui.separator();
            ui.strong("Tilt Key Bindings");
            if ui.button("Reset tilt keys to WASD").clicked() {
                settings.tilt_key_bindings.set_wasd_defaults();
            }
            egui::Grid::new("tilt_bindings")
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for action in [
                        TiltBindingAction::Up,
                        TiltBindingAction::Down,
                        TiltBindingAction::Left,
                        TiltBindingAction::Right,
                    ] {
                        ui.label(tilt_label(action));
                        let key_name =
                            format!("{:?}", settings.tilt_key_bindings.get(action));
                        let capture_label =
                            if state.rebinding_action == Some(InputBindingAction::Tilt(action))
                            {
                                format!("Press key... ({key_name})")
                            } else {
                                key_name
                            };
                        if ui.button(capture_label).clicked() {
                            state.rebinding_action =
                                Some(InputBindingAction::Tilt(action));
                        }
                        ui.end_row();
                    }
                });
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

fn tilt_label(action: TiltBindingAction) -> &'static str {
    match action {
        TiltBindingAction::Up => "Tilt Up",
        TiltBindingAction::Down => "Tilt Down",
        TiltBindingAction::Left => "Tilt Left",
        TiltBindingAction::Right => "Tilt Right",
    }
}

fn gamepad_action_label(action: GamepadAction) -> &'static str {
    match action {
        GamepadAction::SpeedUp => "Speed-up (hold)",
        GamepadAction::Rewind => "Rewind (hold)",
        GamepadAction::Pause => "Pause (toggle)",
        GamepadAction::Turbo => "Turbo / rapid-fire (hold)",
    }
}

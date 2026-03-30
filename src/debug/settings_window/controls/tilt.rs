use crate::debug::DebugWindowState;
use crate::settings::{InputBindingAction, LeftStickMode, Settings, TiltBindingAction, TiltInputMode};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    egui::CollapsingHeader::new("MBC7 Tilt")
        .default_open(false)
        .show(ui, |ui| {
            egui::ComboBox::from_label("Left stick behavior")
                .selected_text(match settings.tilt.left_stick_mode {
                    LeftStickMode::Auto => "Auto (Tilt on MBC7, D-pad otherwise)",
                    LeftStickMode::Tilt => "Always Tilt",
                    LeftStickMode::Dpad => "Always D-pad",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.tilt.left_stick_mode,
                        LeftStickMode::Auto,
                        "Auto (Tilt on MBC7, D-pad otherwise)",
                    );
                    ui.selectable_value(
                        &mut settings.tilt.left_stick_mode,
                        LeftStickMode::Tilt,
                        "Always Tilt",
                    );
                    ui.selectable_value(
                        &mut settings.tilt.left_stick_mode,
                        LeftStickMode::Dpad,
                        "Always D-pad",
                    );
                });
            egui::ComboBox::from_label("Tilt input source")
                .selected_text(match settings.tilt.input_mode {
                    TiltInputMode::Keyboard => "Keyboard (WASD)",
                    TiltInputMode::Mouse => "Mouse",
                    TiltInputMode::Auto => "Auto-detect",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.tilt.input_mode,
                        TiltInputMode::Keyboard,
                        "Keyboard (WASD)",
                    );
                    ui.selectable_value(
                        &mut settings.tilt.input_mode,
                        TiltInputMode::Mouse,
                        "Mouse",
                    );
                    ui.selectable_value(
                        &mut settings.tilt.input_mode,
                        TiltInputMode::Auto,
                        "Auto-detect",
                    );
                });
            ui.checkbox(&mut settings.tilt.invert_x, "Invert tilt X");
            ui.checkbox(&mut settings.tilt.invert_y, "Invert tilt Y");
            ui.checkbox(
                &mut settings.tilt.stick_bypass_lerp,
                "Direct left-stick tilt (bypass lerp)",
            );
            ui.add(
                egui::Slider::new(&mut settings.tilt.sensitivity, 0.1..=3.0)
                    .text("Tilt sensitivity"),
            );
            ui.add(
                egui::Slider::new(&mut settings.tilt.lerp, 0.0..=1.0).text("Tilt smoothing"),
            );
            ui.add(
                egui::Slider::new(&mut settings.tilt.deadzone, 0.0..=0.5).text("Tilt deadzone"),
            );

            ui.separator();
            ui.strong("Tilt Key Bindings");
            if ui.button("Reset tilt keys to WASD").clicked() {
                settings.tilt.key_bindings.set_wasd_defaults();
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
                            format!("{:?}", settings.tilt.key_bindings.get(action));
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

pub(super) fn tilt_label(action: TiltBindingAction) -> &'static str {
    match action {
        TiltBindingAction::Up => "Tilt Up",
        TiltBindingAction::Down => "Tilt Down",
        TiltBindingAction::Left => "Tilt Left",
        TiltBindingAction::Right => "Tilt Right",
    }
}


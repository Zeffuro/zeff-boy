use super::super::{
    layout,
    search::{self, SettingId},
};
use crate::debug::DebugWindowState;
use crate::settings::{
    BindingTarget, GameplayBindingSource, GameplayTransform, InputScope, LeftStickMode, Settings,
    TiltBindingAction, TiltInputMode, TransformValue,
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    let reveal = [
        SettingId::InputDevicesLeftStickBehavior,
        SettingId::InputDevicesTiltInputSource,
        SettingId::InputDevicesInvertTiltX,
        SettingId::InputDevicesInvertTiltY,
        SettingId::InputDevicesDirectLeftStickTilt,
        SettingId::InputDevicesTiltSensitivity,
        SettingId::InputDevicesTiltSmoothing,
        SettingId::InputDevicesTiltDeadzone,
        SettingId::InputDevicesTiltKeyBindings,
        SettingId::InputDevicesResetTiltKeysToWasd,
        SettingId::InputDevicesTiltUpKey,
        SettingId::InputDevicesTiltDownKey,
        SettingId::InputDevicesTiltLeftKey,
        SettingId::InputDevicesTiltRightKey,
    ]
    .iter()
    .any(|&id| search::requested(ui, id));
    egui::CollapsingHeader::new("Stick behavior & MBC7 tilt")
        .open(reveal.then_some(true))
        .show(ui, |ui| {
            let scope = state.settings_ui.input_scope.clone();
            for (field, id) in [
                (
                    GameplayTransform::LeftStickMode,
                    SettingId::InputDevicesLeftStickBehavior,
                ),
                (
                    GameplayTransform::InputMode,
                    SettingId::InputDevicesTiltInputSource,
                ),
                (
                    GameplayTransform::InvertX,
                    SettingId::InputDevicesInvertTiltX,
                ),
                (
                    GameplayTransform::InvertY,
                    SettingId::InputDevicesInvertTiltY,
                ),
                (
                    GameplayTransform::StickBypassLerp,
                    SettingId::InputDevicesDirectLeftStickTilt,
                ),
                (
                    GameplayTransform::Sensitivity,
                    SettingId::InputDevicesTiltSensitivity,
                ),
                (
                    GameplayTransform::Smoothing,
                    SettingId::InputDevicesTiltSmoothing,
                ),
                (
                    GameplayTransform::Deadzone,
                    SettingId::InputDevicesTiltDeadzone,
                ),
            ] {
                transform_row(ui, settings, &scope, field, id);
            }
            ui.separator();
            let response = ui.strong("Tilt key bindings");
            search::target(ui, SettingId::InputDevicesTiltKeyBindings, &response);
            let response = ui.button(if scope == InputScope::Global {
                "Reset tilt keys to WASD"
            } else {
                "Use inherited tilt keys"
            });
            search::target(ui, SettingId::InputDevicesResetTiltKeysToWasd, &response);
            if response.clicked() {
                super::joypad::clear_capture(state);
                for action in [
                    TiltBindingAction::Up,
                    TiltBindingAction::Down,
                    TiltBindingAction::Left,
                    TiltBindingAction::Right,
                ] {
                    let target = BindingTarget::Tilt(action);
                    if scope == InputScope::Global {
                        let value = Settings::default()
                            .binding(&scope, target, GameplayBindingSource::Keyboard)
                            .value;
                        let _ = settings.set_binding(
                            &scope,
                            target,
                            GameplayBindingSource::Keyboard,
                            value,
                        );
                    } else {
                        settings.inherit_binding(&scope, target, GameplayBindingSource::Keyboard);
                    }
                }
            }
            for (action, id) in [
                (TiltBindingAction::Up, SettingId::InputDevicesTiltUpKey),
                (TiltBindingAction::Down, SettingId::InputDevicesTiltDownKey),
                (TiltBindingAction::Left, SettingId::InputDevicesTiltLeftKey),
                (
                    TiltBindingAction::Right,
                    SettingId::InputDevicesTiltRightKey,
                ),
            ] {
                let target = BindingTarget::Tilt(action);
                let binding = settings.binding_set(&scope, target, GameplayBindingSource::Keyboard);
                let label = super::joypad::binding_set_label(binding.value.as_ref());
                layout::row(ui, id, Some(tilt_label(action)), |ui| {
                    let response = ui.add_sized([170.0, 32.0], egui::Button::new(label));
                    if response.clicked() {
                        super::joypad::open_binding_editor(
                            settings,
                            state,
                            target,
                            GameplayBindingSource::Keyboard,
                            tilt_label(action).to_owned(),
                        );
                    }
                    response
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "From {}",
                            super::scope_label(&binding.origin)
                        ))
                        .weak(),
                    );
                    let hotkey_conflicts = super::joypad::global_hotkey_conflicts_set(
                        settings,
                        binding.value.as_ref(),
                    );
                    if !hotkey_conflicts.is_empty() {
                        ui.label(
                            egui::RichText::new("!").color(egui::Color32::from_rgb(240, 180, 70)),
                        )
                        .on_hover_text(format!(
                            "Also assigned to global {}.",
                            hotkey_conflicts.join(", ")
                        ));
                    }
                    if ui.small_button("Unbind").clicked() {
                        super::joypad::clear_capture(state);
                        let _ = settings.set_binding(
                            &scope,
                            target,
                            GameplayBindingSource::Keyboard,
                            None,
                        );
                    }
                    if scope != InputScope::Global
                        && ui
                            .add_enabled(
                                binding.origin == scope,
                                egui::Button::new("Use inherited").small(),
                            )
                            .clicked()
                    {
                        super::joypad::clear_capture(state);
                        settings.inherit_binding(&scope, target, GameplayBindingSource::Keyboard);
                    }
                });
            }
        });
}

pub(super) fn transform_row(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    scope: &InputScope,
    field: GameplayTransform,
    id: SettingId,
) {
    let resolved = settings.transform(scope, field);
    let mut value = resolved.value.clone();
    let response = layout::row(ui, id, None, |ui| match &mut value {
        TransformValue::Bool(value) => ui.checkbox(value, ""),
        TransformValue::Float(value) => {
            let range = match field {
                GameplayTransform::Sensitivity => 0.1..=3.0,
                GameplayTransform::Deadzone => 0.0..=0.5,
                _ => 0.0..=1.0,
            };
            ui.add(egui::Slider::new(value, range).step_by(0.01))
        }
        TransformValue::InputMode(value) => {
            egui::ComboBox::from_id_salt(id)
                .width(240.0)
                .selected_text(match value {
                    TiltInputMode::Keyboard => "Keyboard",
                    TiltInputMode::Mouse => "Mouse",
                    TiltInputMode::Auto => "Auto-detect",
                })
                .show_ui(ui, |ui| {
                    for (choice, label) in [
                        (TiltInputMode::Keyboard, "Keyboard"),
                        (TiltInputMode::Mouse, "Mouse"),
                        (TiltInputMode::Auto, "Auto-detect"),
                    ] {
                        ui.selectable_value(value, choice, label);
                    }
                })
                .response
        }
        TransformValue::LeftStickMode(value) => {
            egui::ComboBox::from_id_salt(id)
                .width(240.0)
                .selected_text(match value {
                    LeftStickMode::Auto => "Auto",
                    LeftStickMode::Tilt => "Always tilt",
                    LeftStickMode::Dpad => "Always D-pad",
                    LeftStickMode::BindingsOnly => "Explicit bindings only",
                })
                .show_ui(ui, |ui| {
                    for (choice, label) in [
                        (LeftStickMode::Auto, "Auto (tilt on MBC7)"),
                        (LeftStickMode::Tilt, "Always tilt"),
                        (LeftStickMode::Dpad, "Always D-pad"),
                        (LeftStickMode::BindingsOnly, "Explicit bindings only"),
                    ] {
                        ui.selectable_value(value, choice, label);
                    }
                })
                .response
        }
    });
    if response.changed() || value != resolved.value {
        let _ = settings.set_transform(scope, field, value);
    }
    if *scope != InputScope::Global {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(format!("From {}", super::scope_label(&resolved.origin)))
                    .weak(),
            );
            if ui
                .add_enabled(
                    resolved.origin == *scope,
                    egui::Button::new("Use inherited").small(),
                )
                .clicked()
            {
                settings.inherit_transform(scope, field);
            }
        });
    }
}

pub(super) fn tilt_label(action: TiltBindingAction) -> &'static str {
    match action {
        TiltBindingAction::Up => "Tilt Up",
        TiltBindingAction::Down => "Tilt Down",
        TiltBindingAction::Left => "Tilt Left",
        TiltBindingAction::Right => "Tilt Right",
    }
}

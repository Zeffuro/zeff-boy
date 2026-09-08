use super::super::{
    ProfileAction, SettingsUiState, layout,
    search::{self, SettingId as Id},
};
use super::scope_label;
use crate::settings::{GamepadAction, InputProfileBindings, Settings};

pub(super) fn draw(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_ui: &mut SettingsUiState,
) -> bool {
    let mut acted = false;
    let scope = settings_ui.input_scope.clone();
    let scope_label = scope_label(&scope);
    let selected = settings_ui.selected_profile_id.as_deref().and_then(|id| {
        settings
            .input_profiles
            .profiles
            .iter()
            .find(|profile| profile.id == id)
    });
    let selected_name = selected.map(|profile| profile.name.clone());
    if search::requested(ui, Id::InputDevicesRenameSelectedProfile) {
        settings_ui.profile_rename = selected_name.clone().unwrap_or_default();
    }
    let differs = selected.is_some_and(|profile| {
        !gameplay_profiles_match(
            &settings.capture_controller_profile(&scope),
            &profile.bindings,
        )
    });
    search::conditional(
        ui,
        Id::InputDevicesApplyInputProfile,
        Id::InputDevicesSaveCurrentAsProfile,
        !settings.input_profiles.profiles.is_empty(),
        "Save a profile before loading it.",
    );
    for id in [
        Id::InputDevicesUpdateSelectedProfile,
        Id::InputDevicesResetSelectedProfile,
        Id::InputDevicesDeleteSelectedProfile,
        Id::InputDevicesRenameSelectedProfile,
        Id::InputDevicesLoadProfileShortcuts,
    ] {
        search::conditional(
            ui,
            id,
            Id::InputDevicesSaveCurrentAsProfile,
            selected_name.is_some(),
            "Save and select a profile before managing it.",
        );
    }

    if settings.input_profiles.profiles.is_empty() {
        ui.label(
            egui::RichText::new(
                "Save the resolved gameplay mappings in this scope to reuse them later.",
            )
            .weak(),
        );
    } else {
        let previous_selection = settings_ui.selected_profile_id.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("Saved profile");
            egui::ComboBox::from_id_salt("input_profile")
                .width(240.0)
                .selected_text(selected_name.as_deref().unwrap_or("Choose a profile"))
                .show_ui(ui, |ui| {
                    for profile in &settings.input_profiles.profiles {
                        ui.selectable_value(
                            &mut settings_ui.selected_profile_id,
                            Some(profile.id.clone()),
                            &profile.name,
                        );
                    }
                })
                .response
        });
        if settings_ui.selected_profile_id != previous_selection {
            settings_ui.profile_rename = settings_ui
                .selected_profile_id
                .as_deref()
                .and_then(|id| {
                    settings
                        .input_profiles
                        .profiles
                        .iter()
                        .find(|profile| profile.id == id)
                })
                .map(|profile| profile.name.clone())
                .unwrap_or_default();
        }
        if layout::row(
            ui,
            Id::InputDevicesApplyInputProfile,
            Some("Gameplay mappings"),
            |ui| {
                ui.add_enabled(
                    selected_name.is_some(),
                    egui::Button::new(format!("Load into {scope_label}")),
                )
            },
        )
        .clicked()
            && let Some((name, bindings)) = selected_bindings(settings, settings_ui)
        {
            acted = true;
            let previous = settings.clone();
            match settings.apply_controller_profile(&scope, &bindings) {
                Ok(()) => {
                    record_undo(settings, settings_ui, previous, "Input profile applied.");
                    settings_ui.profile_notice = Some(format!(
                        "Loaded {name} into {scope_label}. Emulator shortcuts were unchanged."
                    ));
                }
                Err(error) => settings_ui.profile_notice = Some(error),
            }
        }
        if differs {
            ui.label(
                egui::RichText::new(
                    "Resolved gameplay mappings in this scope differ from this profile.",
                )
                .small()
                .weak(),
            );
        }

        if ui
            .button(if settings_ui.profile_manage_open {
                "Hide profile management"
            } else {
                "Manage selected profile"
            })
            .clicked()
        {
            settings_ui.profile_manage_open = !settings_ui.profile_manage_open;
            if settings_ui.profile_manage_open {
                settings_ui.profile_rename = selected_name.clone().unwrap_or_default();
            }
        }
        if settings_ui.profile_manage_open && selected_name.is_some() {
            draw_manage(ui, settings_ui);
        }
    }

    ui.add_space(4.0);
    layout::row(
        ui,
        Id::InputDevicesSaveCurrentAsProfile,
        Some("New profile"),
        |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut settings_ui.profile_name)
                    .hint_text("Name for a new profile")
                    .desired_width(240.0),
            )
        },
    );
    if ui
        .add_enabled(
            !settings_ui.profile_name.trim().is_empty(),
            egui::Button::new(format!("Save copy from {scope_label}")),
        )
        .clicked()
    {
        acted = true;
        let previous = settings.clone();
        match settings.save_controller_profile(&settings_ui.profile_name, &scope) {
            Ok(id) => {
                settings_ui.selected_profile_id = Some(id);
                settings_ui.profile_name.clear();
                record_undo(settings, settings_ui, previous, "Input profile saved.");
                settings_ui.profile_notice = Some(format!(
                    "Saved a copied controller profile from {scope_label}."
                ));
            }
            Err(error) => settings_ui.profile_notice = Some(error),
        }
    }
    ui.label(
        egui::RichText::new(format!(
            "Controller profiles are copied into {scope_label}; they do not stay linked."
        ))
        .small()
        .weak(),
    );

    if let Some(action) = settings_ui.profile_action_confirmation.clone() {
        draw_confirmation(ui, settings, settings_ui, action, &scope, &mut acted);
    }
    draw_delete_confirmation(ui, settings, settings_ui, &mut acted);

    if let Some(message) = &settings_ui.profile_notice {
        ui.label(egui::RichText::new(message).small().weak());
    }
    acted
}

fn selected_bindings(
    settings: &Settings,
    settings_ui: &SettingsUiState,
) -> Option<(String, InputProfileBindings)> {
    settings_ui.selected_profile_id.as_deref().and_then(|id| {
        settings
            .input_profiles
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .map(|profile| (profile.name.clone(), profile.bindings.clone()))
    })
}

fn gameplay_profiles_match(current: &InputProfileBindings, saved: &InputProfileBindings) -> bool {
    let mut current = current.clone();
    current.shortcuts = saved.shortcuts.clone();
    current.speedup_key = saved.speedup_key.clone();
    current.rewind_key = saved.rewind_key.clone();
    for action in [
        GamepadAction::SpeedUp,
        GamepadAction::Rewind,
        GamepadAction::Pause,
        GamepadAction::Turbo,
    ] {
        current
            .gamepad
            .set_action(action, saved.gamepad.get_action(action));
    }
    let mut current_settings = Settings::default();
    current_settings.apply_input_profile(&current);
    let mut saved_settings = Settings::default();
    saved_settings.apply_input_profile(saved);
    let mut current = current_settings.resolve_gameplay_input(&crate::settings::InputScope::Global);
    let saved = saved_settings.resolve_gameplay_input(&crate::settings::InputScope::Global);
    current.tilt.key_bindings = saved.tilt.key_bindings.clone();
    current == saved
}

fn draw_manage(ui: &mut egui::Ui, settings_ui: &mut SettingsUiState) {
    ui.group(|ui| {
        if layout::row(
            ui,
            Id::InputDevicesUpdateSelectedProfile,
            Some("Saved gameplay"),
            |ui| ui.button("Replace with resolved mappings…"),
        )
        .clicked()
        {
            settings_ui.profile_action_confirmation = settings_ui
                .selected_profile_id
                .clone()
                .map(ProfileAction::Update);
        }
        if layout::row(
            ui,
            Id::InputDevicesResetSelectedProfile,
            Some("Saved gameplay"),
            |ui| ui.button("Restore built-in mappings…"),
        )
        .clicked()
        {
            settings_ui.profile_action_confirmation = settings_ui
                .selected_profile_id
                .clone()
                .map(ProfileAction::Reset);
        }
        layout::row(
            ui,
            Id::InputDevicesRenameSelectedProfile,
            Some("Rename profile"),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut settings_ui.profile_rename)
                        .desired_width(240.0),
                )
            },
        );
        if layout::row(ui, Id::InputDevicesRenameSelectedProfile, Some(""), |ui| {
            ui.add_enabled(
                !settings_ui.profile_rename.trim().is_empty(),
                egui::Button::new("Rename…"),
            )
        })
        .clicked()
        {
            settings_ui.profile_action_confirmation = settings_ui
                .selected_profile_id
                .clone()
                .map(ProfileAction::Rename);
        }
        if layout::row(
            ui,
            Id::InputDevicesLoadProfileShortcuts,
            Some("Emulator shortcuts"),
            |ui| ui.button("Load saved emulator shortcuts…"),
        )
        .clicked()
        {
            settings_ui.profile_action_confirmation = settings_ui
                .selected_profile_id
                .clone()
                .map(ProfileAction::Shortcuts);
        }
        if layout::row(
            ui,
            Id::InputDevicesDeleteSelectedProfile,
            Some("Saved profile"),
            |ui| ui.button("Delete profile…"),
        )
        .clicked()
        {
            settings_ui.profile_delete_confirmation = settings_ui.selected_profile_id.clone();
        }
    });
}

fn draw_confirmation(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_ui: &mut SettingsUiState,
    action: ProfileAction,
    scope: &crate::settings::InputScope,
    acted: &mut bool,
) {
    let scope_label = scope_label(scope);
    ui.group(|ui| {
        let (id, label) = match &action {
            ProfileAction::Update(id) => (id.as_str(), format!("Replace this saved profile with the resolved gameplay mappings from {scope_label}? Saved emulator shortcuts will be retained.")),
            ProfileAction::Reset(id) => (id.as_str(), "Reset this saved profile to built-in gameplay mappings? Saved emulator shortcuts will be retained.".to_owned()),
            ProfileAction::Rename(id) => (id.as_str(), format!("Rename this saved profile to \"{}\"?", settings_ui.profile_rename.trim())),
            ProfileAction::Shortcuts(id) => (id.as_str(), "Load this profile’s saved emulator shortcuts globally? This changes shortcuts for every system and game.".to_owned()),
        };
        ui.label(label);
        ui.horizontal(|ui| {
            if ui.button("Confirm").clicked() {
                *acted = true;
                let previous = settings.clone();
                let result = match &action {
                    ProfileAction::Update(_) => settings.update_controller_profile(id, scope),
                    ProfileAction::Reset(_) => settings.reset_input_profile(id),
                    ProfileAction::Rename(_) => rename_profile(settings, id, &settings_ui.profile_rename),
                    ProfileAction::Shortcuts(_) => {
                        let bindings = settings.input_profiles.profiles.iter().find(|profile| profile.id == id)
                            .map(|profile| profile.bindings.clone())
                            .ok_or_else(|| "That input profile no longer exists.".to_owned());
                        if let Ok(bindings) = &bindings {
                            settings.apply_profile_shortcuts(bindings);
                        }
                        bindings.map(|_| ())
                    }
                };
                match result {
                    Ok(()) => {
                        let (undo_label, notice) = match action {
                            ProfileAction::Update(_) => ("Input profile changed.", format!("Saved profile updated from {scope_label}.")),
                            ProfileAction::Reset(_) => ("Input profile changed.", "Saved profile reset to built-in gameplay mappings.".to_owned()),
                            ProfileAction::Rename(_) => ("Input profile renamed.", "Saved profile renamed.".to_owned()),
                            ProfileAction::Shortcuts(_) => ("Emulator shortcuts loaded.", "Loaded saved emulator shortcuts globally.".to_owned()),
                        };
                        record_undo(settings, settings_ui, previous, undo_label);
                        settings_ui.profile_notice = Some(notice);
                    }
                    Err(error) => settings_ui.profile_notice = Some(error),
                }
                settings_ui.profile_action_confirmation = None;
            }
            if ui.button("Cancel").clicked() {
                settings_ui.profile_action_confirmation = None;
            }
        });
    });
}

fn rename_profile(settings: &mut Settings, id: &str, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("Use a profile name of 1–64 characters without control characters.".into());
    }
    if settings
        .input_profiles
        .profiles
        .iter()
        .any(|profile| profile.id != id && profile.name.eq_ignore_ascii_case(name))
    {
        return Err("That profile name already exists. Choose a different name.".into());
    }
    let profile = settings
        .input_profiles
        .profiles
        .iter_mut()
        .find(|profile| profile.id == id)
        .ok_or_else(|| "That input profile no longer exists.".to_owned())?;
    profile.name = name.to_owned();
    Ok(())
}

fn draw_delete_confirmation(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_ui: &mut SettingsUiState,
    acted: &mut bool,
) {
    if let Some(id) = settings_ui.profile_delete_confirmation.clone() {
        ui.group(|ui| {
            ui.label("Delete this saved profile? Current live mappings will not change.");
            ui.horizontal(|ui| {
                if ui.button("Delete profile").clicked() {
                    *acted = true;
                    let previous = settings.clone();
                    if settings.delete_input_profile(&id) {
                        settings_ui.selected_profile_id = None;
                        settings_ui.profile_manage_open = false;
                        record_undo(settings, settings_ui, previous, "Input profile deleted.");
                        settings_ui.profile_notice = Some("Deleted input profile.".into());
                    }
                    settings_ui.profile_delete_confirmation = None;
                }
                if ui.button("Cancel").clicked() {
                    settings_ui.profile_delete_confirmation = None;
                }
            });
        });
    }
}

fn record_undo(
    settings: &Settings,
    settings_ui: &mut SettingsUiState,
    previous: Settings,
    label: &str,
) {
    settings_ui.undo = Some(previous);
    settings_ui.undo_baseline = Some(settings.clone());
    settings_ui.undo_label = Some(label.into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{
        BindingAction, BindingTarget, GameplayBindingSource, InputScope, PhysicalBinding,
    };
    use winit::keyboard::KeyCode;

    #[test]
    fn legacy_profile_matches_its_resolved_copy_and_ignores_shortcuts() {
        let settings = Settings::default();
        let saved = settings.capture_input_profile();
        let mut current = settings.capture_controller_profile(&InputScope::Global);
        current.speedup_key = "KeyQ".into();
        current.gamepad.set_action(GamepadAction::Pause, "North");
        assert!(gameplay_profiles_match(&current, &saved));
    }

    #[test]
    fn profile_comparison_distinguishes_effective_unbound_and_bound_values() {
        let mut settings = Settings::default();
        let target = BindingTarget::Joypad {
            player: 1,
            action: BindingAction::A,
        };
        let saved = settings.capture_input_profile();
        settings
            .set_binding(
                &InputScope::Global,
                target,
                GameplayBindingSource::Keyboard,
                None,
            )
            .unwrap();
        let unbound = settings.capture_input_profile();
        assert!(!gameplay_profiles_match(&unbound, &saved));
        settings
            .set_binding(
                &InputScope::Global,
                target,
                GameplayBindingSource::Keyboard,
                Some(PhysicalBinding::Keyboard(KeyCode::KeyQ)),
            )
            .unwrap();
        settings
            .set_binding(
                &InputScope::Global,
                target,
                GameplayBindingSource::Keyboard,
                None,
            )
            .unwrap();
        assert!(gameplay_profiles_match(
            &settings.capture_input_profile(),
            &unbound
        ));
    }
}

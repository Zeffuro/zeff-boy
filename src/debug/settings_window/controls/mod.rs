pub(super) mod controller_diagram;
mod gamepad_actions;
mod joypad;
mod shortcuts;
pub(super) mod tilt;
mod wonderswan;
use super::{InputDevicesPage, SettingsUiState};
use crate::debug::DebugWindowState;
use crate::emu_backend::ActiveSystem;
use crate::settings::{GamepadAssignment, Settings};
use controller_diagram::DiagramKind;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum BindingSource {
    #[default]
    Controller,
    Keyboard,
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    active_system: Option<ActiveSystem>,
) {
    let previous_page = state.settings_ui.input_page;
    ui.horizontal_wrapped(|ui| {
        for page in InputDevicesPage::ALL {
            ui.selectable_value(&mut state.settings_ui.input_page, page, page.label());
        }
    });
    if previous_page != state.settings_ui.input_page {
        cancel_capture(state);
    }
    ui.add_space(8.0);
    draw_capture_status(ui, state);
    match state.settings_ui.input_page {
        InputDevicesPage::Controls => draw_controller_workspace(ui, settings, state, active_system),
        InputDevicesPage::Hotkeys => {
            ui.label("Shortcuts control the emulator, independently of the controller layout.");
            ui.add_space(8.0);
            shortcuts::draw(ui, settings, state);
            gamepad_actions::draw(ui, settings, state);
        }
        InputDevicesPage::TestAndCalibrate => {
            draw_test_and_calibrate(ui, settings, &mut state.settings_ui);
            ui.add_space(8.0);
            tilt::draw(ui, settings, state);
        }
    }
}

fn draw_controller_workspace(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    active_system: Option<ActiveSystem>,
) {
    let automatic_layout = match active_system {
        Some(ActiveSystem::Pce)
            if settings.emulation.pce_controller
                == crate::settings::PceControllerPreference::SixButton =>
        {
            DiagramKind::PceSixButton
        }
        Some(system) => DiagramKind::for_system(system),
        None => DiagramKind::StandardGamepad,
    };
    let previous_layout = state.settings_ui.controller_layout;
    state.settings_ui.selected_player = state.settings_ui.selected_player.clamp(1, 5);
    let narrow = ui.available_width() < 550.0;
    ui.horizontal_wrapped(|ui| {
        ui.label("Player");
        egui::ComboBox::from_id_salt("controls_player")
            .width(145.0)
            .selected_text(format!("Player {}", state.settings_ui.selected_player))
            .show_ui(ui, |ui| {
                let max_player = if state
                    .settings_ui
                    .controller_layout
                    .unwrap_or(automatic_layout)
                    == DiagramKind::WonderSwan
                {
                    1
                } else {
                    5
                };
                for player in 1..=max_player {
                    let label = format!("Player {player}");
                    if ui
                        .selectable_value(&mut state.settings_ui.selected_player, player, label)
                        .changed()
                    {
                        joypad::clear_capture(state);
                    }
                }
            });
        if narrow {
            ui.end_row();
        }
        ui.label("Button layout");
        egui::ComboBox::from_id_salt("controls_layout")
            .width(190.0)
            .selected_text(state.settings_ui.controller_layout.map_or_else(
                || format!("Automatic · {}", automatic_layout.label()),
                |layout| layout.label().to_owned(),
            ))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut state.settings_ui.controller_layout,
                    None,
                    "Automatic (current game)",
                );
                for layout in DiagramKind::ALL {
                    ui.selectable_value(
                        &mut state.settings_ui.controller_layout,
                        Some(layout),
                        layout.label(),
                    );
                }
            });
    });
    ui.add_space(6.0);
    let kind = state
        .settings_ui
        .controller_layout
        .unwrap_or(automatic_layout);
    if previous_layout != state.settings_ui.controller_layout {
        cancel_capture(state);
    }
    reconcile_layout(state, kind);
    draw_player_connection(ui, settings, &mut state.settings_ui);
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        for (source, label) in [
            (BindingSource::Controller, "Controller"),
            (BindingSource::Keyboard, "Keyboard"),
        ] {
            if ui
                .selectable_value(&mut state.settings_ui.binding_source, source, label)
                .changed()
            {
                joypad::clear_capture(state);
            }
        }
        ui.label(egui::RichText::new("Select a button or mapping to change it.").weak());
    });
    ui.add_space(8.0);
    let player = state.settings_ui.selected_player;
    let source = state.settings_ui.binding_source;
    if ui.available_width() >= 720.0 {
        ui.columns(2, |columns| {
            draw_controller_reference(&mut columns[0], state, kind);
            draw_mapping_table(&mut columns[1], settings, state, player, source, kind);
        });
    } else {
        draw_controller_reference(ui, state, kind);
        ui.add_space(8.0);
        draw_mapping_table(ui, settings, state, player, source, kind);
    }
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(
            "These mappings apply across games. Button layout changes the visual guide.",
        )
        .weak(),
    );
    if player >= 3 {
        ui.label("Additional players depend on the game and connected controller ports.");
    }
    ui.add_space(8.0);
    let mut profile_action = false;
    egui::CollapsingHeader::new(format!(
        "Saved input profiles ({})",
        settings.input_profiles.profiles.len()
    ))
    .id_salt("saved_input_profiles")
    .show(ui, |ui| {
        profile_action = draw_profiles(ui, settings, &mut state.settings_ui)
    });
    if profile_action {
        cancel_capture(state);
    }
}

fn draw_controller_reference(ui: &mut egui::Ui, state: &mut DebugWindowState, kind: DiagramKind) {
    let player = state.settings_ui.selected_player;
    let selected = joypad::captured_action(state, player);
    let pressed = state.settings_ui.gamepad_snapshot.players[usize::from(player - 1)].buttons;
    if let Some(action) = controller_diagram::draw(ui, kind, pressed, selected) {
        joypad::begin_capture(state, player, state.settings_ui.binding_source, action);
    }
}

fn draw_mapping_table(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    player: u8,
    source: BindingSource,
    kind: DiagramKind,
) {
    if kind == DiagramKind::WonderSwan {
        wonderswan::draw_focused(ui, settings, state, source);
    } else {
        joypad::draw_focused(ui, settings, state, player, source, kind);
    }
}

fn draw_player_connection(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_ui: &mut SettingsUiState,
) {
    let index = usize::from(settings_ui.selected_player - 1);
    let runtime = &settings_ui.gamepad_snapshot.players[index];
    let assignment = &settings.input_devices.players[index];
    let selected_label = match assignment {
        GamepadAssignment::Auto => "Automatic".to_owned(),
        GamepadAssignment::Disabled => "Disabled".to_owned(),
        GamepadAssignment::Reserved(fingerprint) => fingerprint.name.clone(),
    };
    let mut choice = None;
    egui::Frame::group(ui.style()).inner_margin(10.0).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("Input device");
            egui::ComboBox::from_id_salt("player_device")
                .width(225.0)
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(matches!(assignment, GamepadAssignment::Auto), "Automatic (recommended)").clicked() {
                        choice = Some((GamepadAssignment::Auto, None));
                    }
                    if ui.selectable_label(matches!(assignment, GamepadAssignment::Disabled), "Disable controller for this player").clicked() {
                        choice = Some((GamepadAssignment::Disabled, None));
                    }
                    for (ordinal, device) in settings_ui.gamepad_snapshot.devices.iter().enumerate() {
                        let selected = matches!(assignment, GamepadAssignment::Reserved(fingerprint) if *fingerprint == device.fingerprint) && runtime.device == Some(device.id);
                        if ui.selectable_label(selected, format!("{} #{}", device.fingerprint.name, ordinal + 1)).clicked() {
                            choice = Some((GamepadAssignment::Reserved(device.fingerprint.clone()), Some(device.id)));
                        }
                    }
                });
            let connected = runtime.device.and_then(|id| settings_ui.gamepad_snapshot.devices.iter().find(|device| device.id == id));
            if let Some(device) = connected {
                ui.label(egui::RichText::new(format!("Connected · {}", device.fingerprint.name)).color(ui.visuals().selection.stroke.color));
            } else {
                ui.label(player_status_label(runtime.status));
            }
        });
        ui.label(egui::RichText::new("Automatic assigns an available controller. Default mappings work without setup; keyboard input remains available too.").weak());
    });
    if let Some((assignment, device)) = choice {
        settings.input_devices.players[index] = assignment;
        if let Some(device) = device {
            settings_ui
                .gamepad_commands
                .push(crate::input::GamepadCommand::Identify {
                    player: settings_ui.selected_player,
                    device,
                });
        }
    }
}

fn player_status_label(status: crate::input::GamepadAssignmentStatus) -> &'static str {
    match status {
        crate::input::GamepadAssignmentStatus::Disabled => "Controller disabled",
        crate::input::GamepadAssignmentStatus::Waiting => "No controller assigned · keyboard ready",
        crate::input::GamepadAssignmentStatus::Ambiguous => {
            "Choose which matching controller to use"
        }
        crate::input::GamepadAssignmentStatus::Connected => "Connected",
    }
}
fn draw_profiles(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_ui: &mut SettingsUiState,
) -> bool {
    let mut acted = false;
    let selected = settings_ui.selected_profile_id.as_deref().and_then(|id| {
        settings
            .input_profiles
            .profiles
            .iter()
            .find(|profile| profile.id == id)
    });
    let selected_name = selected.map(|profile| profile.name.clone());
    let differs =
        selected.is_some_and(|profile| profile.bindings != settings.capture_input_profile());
    if settings.input_profiles.profiles.is_empty() {
        ui.label(egui::RichText::new("Save your current mappings to reuse them later.").weak());
    } else {
        ui.horizontal_wrapped(|ui| {
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
                });
            if ui
                .add_enabled(selected_name.is_some(), egui::Button::new("Load profile"))
                .clicked()
            {
                let selected = settings_ui.selected_profile_id.as_deref().and_then(|id| {
                    settings
                        .input_profiles
                        .profiles
                        .iter()
                        .find(|profile| profile.id == id)
                        .map(|profile| (profile.name.clone(), profile.bindings.clone()))
                });
                if let Some((name, bindings)) = selected {
                    acted = true;
                    let previous = settings.clone();
                    settings.apply_input_profile(&bindings);
                    settings_ui.undo = Some(previous);
                    settings_ui.undo_baseline = Some(settings.clone());
                    settings_ui.undo_label = Some("Input profile applied.".into());
                    settings_ui.profile_notice = Some(format!("Loaded {name}."));
                }
            }
            ui.add_enabled_ui(selected_name.is_some(), |ui| {
                ui.menu_button("Manage", |ui| {
                    if ui.button("Replace with current mappings…").clicked() {
                        settings_ui.profile_action_confirmation = settings_ui
                            .selected_profile_id
                            .clone()
                            .map(super::ProfileAction::Update);
                        ui.close();
                    }
                    if ui.button("Restore built-in mappings…").clicked() {
                        settings_ui.profile_action_confirmation = settings_ui
                            .selected_profile_id
                            .clone()
                            .map(super::ProfileAction::Reset);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Delete profile…").clicked() {
                        settings_ui.profile_delete_confirmation =
                            settings_ui.selected_profile_id.clone();
                        ui.close();
                    }
                });
            });
        });
        if differs {
            ui.label(
                egui::RichText::new("Current mappings differ from this profile.")
                    .small()
                    .weak(),
            );
        }
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut settings_ui.profile_name)
                .hint_text("Name for a new profile")
                .desired_width(240.0),
        );
        if ui
            .add_enabled(
                !settings_ui.profile_name.trim().is_empty(),
                egui::Button::new("Save as new profile"),
            )
            .clicked()
        {
            acted = true;
            let previous = settings.clone();
            match settings.save_input_profile(&settings_ui.profile_name) {
                Ok(id) => {
                    settings_ui.selected_profile_id = Some(id);
                    settings_ui.profile_name.clear();
                    settings_ui.undo = Some(previous);
                    settings_ui.undo_baseline = Some(settings.clone());
                    settings_ui.undo_label = Some("Input profile saved.".into());
                    settings_ui.profile_notice = Some("Saved input profile.".into());
                }
                Err(error) => settings_ui.profile_notice = Some(error),
            }
        }
    });

    if let Some(action) = settings_ui.profile_action_confirmation.clone() {
        ui.group(|ui| {
            let (id, label) = match &action {
                super::ProfileAction::Update(id) => (
                    id.as_str(),
                    "Update this saved profile from current bindings?",
                ),

                super::ProfileAction::Reset(id) => (
                    id.as_str(),
                    "Reset this saved profile to built-in bindings? Live mappings will not change.",
                ),
            };

            ui.label(label);

            ui.horizontal(|ui| {
                if ui.button("Confirm").clicked() {
                    acted = true;
                    let previous = settings.clone();

                    let result = match &action {
                        super::ProfileAction::Update(_) => settings.update_input_profile(id),

                        super::ProfileAction::Reset(_) => settings.reset_input_profile(id),
                    };

                    match result {
                        Ok(()) => {
                            settings_ui.undo = Some(previous);

                            settings_ui.undo_baseline = Some(settings.clone());

                            let message = match action {
                                super::ProfileAction::Update(_) => "Saved profile updated.",

                                super::ProfileAction::Reset(_) => {
                                    "Saved profile reset to built-in bindings."
                                }
                            };

                            settings_ui.undo_label = Some("Input profile changed.".to_string());

                            settings_ui.profile_notice = Some(message.to_string());
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

    if let Some(id) = settings_ui.profile_delete_confirmation.clone() {
        ui.group(|ui| {
            ui.label("Delete this saved profile? Current live bindings will not change.");

            ui.horizontal(|ui| {
                if ui.button("Delete profile").clicked() {
                    acted = true;
                    let previous = settings.clone();

                    if settings.delete_input_profile(&id) {
                        settings_ui.selected_profile_id = None;

                        settings_ui.undo = Some(previous);

                        settings_ui.undo_baseline = Some(settings.clone());

                        settings_ui.undo_label = Some("Input profile deleted.".to_string());

                        settings_ui.profile_notice = Some("Deleted input profile.".to_string());
                    }

                    settings_ui.profile_delete_confirmation = None;
                }

                if ui.button("Cancel").clicked() {
                    settings_ui.profile_delete_confirmation = None;
                }
            });
        });
    }

    if let Some(message) = &settings_ui.profile_notice {
        ui.label(egui::RichText::new(message).small().weak());
    }
    acted
}

fn draw_capture_status(ui: &mut egui::Ui, state: &mut DebugWindowState) {
    let capturing = state.rebinding_action.is_some()
        || state.rebinding_shortcut.is_some()
        || state.rebinding_gamepad.is_some()
        || state.rebinding_gamepad_p2.is_some()
        || state.rebinding_gamepad_pce_multitap.is_some()
        || state.rebinding_ws_gamepad.is_some()
        || state.rebinding_gamepad_action.is_some()
        || state.rebinding_speedup
        || state.rebinding_rewind;

    if capturing {
        ui.separator();

        ui.group(|ui| {
            let message = if state.settings_ui.gamepad_snapshot.capture_active
                && !state.settings_ui.gamepad_snapshot.capture_ready
            {
                "Release held controller inputs before capture can begin…"
            } else {
                "Waiting for a control input…"
            };

            ui.label(egui::RichText::new(message).color(egui::Color32::YELLOW));

            if ui.button("Cancel capture").clicked() {
                state.rebinding_action = None;

                state.rebinding_shortcut = None;

                state.rebinding_gamepad = None;

                state.rebinding_gamepad_p2 = None;

                state.rebinding_gamepad_pce_multitap = None;

                state.rebinding_ws_gamepad = None;

                state.rebinding_gamepad_action = None;

                state.rebinding_speedup = false;

                state.rebinding_rewind = false;
            }
        });
    }
}

fn draw_test_and_calibrate(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_ui: &mut SettingsUiState,
) {
    ui.label("Move either stick or press a button to inspect live input.");
    ui.add_space(8.0);
    let devices = &settings_ui.gamepad_snapshot.devices;
    if !devices
        .iter()
        .any(|device| Some(device.id) == settings_ui.test_device)
    {
        settings_ui.test_device = devices.first().map(|device| device.id);
    }
    if devices.is_empty() {
        ui.group(|ui| {
            ui.label("Connect a controller to test its buttons and sticks.");
        });
        return;
    }
    let selected_name = devices
        .iter()
        .enumerate()
        .find(|(_, device)| Some(device.id) == settings_ui.test_device)
        .map(|(index, device)| format!("{} #{}", device.fingerprint.name, index + 1))
        .unwrap_or_default();
    egui::ComboBox::from_id_salt("test_input_device")
        .width(300.0)
        .selected_text(selected_name)
        .show_ui(ui, |ui| {
            for (index, device) in devices.iter().enumerate() {
                ui.selectable_value(
                    &mut settings_ui.test_device,
                    Some(device.id),
                    format!("{} #{}", device.fingerprint.name, index + 1),
                );
            }
        });
    if let Some(device) = devices
        .iter()
        .find(|device| Some(device.id) == settings_ui.test_device)
    {
        egui::Frame::group(ui.style())
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    stick_plot(
                        ui,
                        "Left stick",
                        device.left_stick,
                        Some(settings.tilt.deadzone),
                    );
                    ui.add_space(12.0);
                    stick_plot(ui, "Right stick", device.right_stick, None);
                });
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Buttons");
                    if device.buttons.is_empty() {
                        ui.label(egui::RichText::new("No buttons pressed").weak());
                    } else {
                        for button in &device.buttons {
                            ui.label(
                                egui::RichText::new(button)
                                    .color(ui.visuals().selection.stroke.color),
                            );
                        }
                    }
                });
                if device.waiting_for_neutral {
                    ui.label(
                        egui::RichText::new("Release the controls to resume gameplay input.")
                            .weak(),
                    );
                }
            });
    }
    ui.add_space(8.0);
    ui.add(
        egui::Slider::new(&mut settings.tilt.deadzone, 0.0..=0.5)
            .text("Left-stick deadzone")
            .step_by(0.01),
    );
    ui.add_space(8.0);
    egui::CollapsingHeader::new("Player assignments").show(ui, |ui| {
        egui::Grid::new("test_player_assignments")
            .num_columns(3)
            .spacing([14.0, 8.0])
            .show(ui, |ui| {
                for (index, player) in settings_ui.gamepad_snapshot.players.iter().enumerate() {
                    ui.label(format!("Player {}", index + 1));
                    ui.label(player_status_label(player.status));
                    ui.label(mapped_button_labels(player.buttons));
                    ui.end_row();
                }
            });
    });
}

fn stick_plot(ui: &mut egui::Ui, title: &str, value: (f32, f32), deadzone: Option<f32>) {
    ui.vertical(|ui| {
        ui.set_width(172.0);
        ui.label(egui::RichText::new(title).strong());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 160.0), egui::Sense::hover());
        let painter = ui.painter();
        let center = rect.center();
        let radius = 69.0;
        let grid = ui.visuals().widgets.noninteractive.bg_stroke;
        painter.circle(center, radius, ui.visuals().extreme_bg_color, grid);
        if let Some(deadzone) = deadzone {
            painter.circle(
                center,
                radius * deadzone.clamp(0.0, 0.5),
                ui.visuals().faint_bg_color,
                grid,
            );
        }
        painter.line_segment(
            [
                center - egui::vec2(radius, 0.0),
                center + egui::vec2(radius, 0.0),
            ],
            grid,
        );
        painter.line_segment(
            [
                center - egui::vec2(0.0, radius),
                center + egui::vec2(0.0, radius),
            ],
            grid,
        );
        let position =
            center + egui::vec2(value.0.clamp(-1.0, 1.0), -value.1.clamp(-1.0, 1.0)) * radius;
        painter.line_segment(
            [center, position],
            egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
        );
        painter.circle(
            position,
            5.0,
            ui.visuals().selection.stroke.color,
            egui::Stroke::NONE,
        );
        ui.label(
            egui::RichText::new(format!("X {:+.2}   Y {:+.2}", value.0, value.1))
                .monospace()
                .size(13.0),
        );
    });
}

fn mapped_button_labels(buttons: u16) -> String {
    let labels: Vec<_> = crate::input::HostButton::WITH_SIX_BUTTONS
        .iter()
        .copied()
        .filter(|button| buttons & button.host_mask_bit() != 0)
        .map(crate::input::HostButton::label)
        .collect();

    if labels.is_empty() {
        "No mapped buttons pressed".to_owned()
    } else {
        format!("Mapped: {}", labels.join(", "))
    }
}

pub(super) fn cancel_capture(state: &mut DebugWindowState) {
    joypad::clear_capture(state);
}

fn reconcile_layout(state: &mut DebugWindowState, kind: DiagramKind) {
    if state.settings_ui.last_controller_layout != Some(kind) {
        cancel_capture(state);
        state.settings_ui.last_controller_layout = Some(kind);
    }
    if kind == DiagramKind::WonderSwan && state.settings_ui.selected_player != 1 {
        state.settings_ui.selected_player = 1;
        cancel_capture(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{BindingAction, InputBindingAction, WonderSwanButton};

    #[test]
    fn automatic_system_layout_change_cancels_capture_and_normalizes_wonderswan_player() {
        let mut state = DebugWindowState::new();
        reconcile_layout(&mut state, DiagramKind::GameBoy);
        state.settings_ui.selected_player = 3;
        state.rebinding_gamepad_pce_multitap = Some((3, BindingAction::A));
        reconcile_layout(&mut state, DiagramKind::WonderSwan);
        assert_eq!(state.settings_ui.selected_player, 1);
        assert!(state.rebinding_gamepad_pce_multitap.is_none());
        assert!(state.settings_ui.controller_layout.is_none());
    }

    #[test]
    fn diagram_capture_replaces_every_previous_capture_family() {
        let mut state = DebugWindowState::new();
        state.rebinding_gamepad_pce_multitap = Some((4, BindingAction::B));
        state.rebinding_rewind = true;
        state.rebinding_gamepad_action = Some(crate::settings::GamepadAction::Turbo);
        joypad::begin_capture(
            &mut state,
            1,
            BindingSource::Keyboard,
            controller_diagram::DiagramAction::WonderSwan(WonderSwanButton::X1),
        );
        assert_eq!(
            state.rebinding_action,
            Some(InputBindingAction::WonderSwan(WonderSwanButton::X1))
        );
        assert!(state.rebinding_gamepad_pce_multitap.is_none());
        assert!(state.rebinding_gamepad_action.is_none());
        assert!(!state.rebinding_rewind);
        joypad::begin_capture(
            &mut state,
            5,
            BindingSource::Controller,
            controller_diagram::DiagramAction::Joypad(BindingAction::A),
        );
        assert_eq!(
            state.rebinding_gamepad_pce_multitap,
            Some((5, BindingAction::A))
        );
        assert!(state.rebinding_action.is_none());
    }
}

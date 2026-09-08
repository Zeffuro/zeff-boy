mod autofire;
pub(crate) mod binding_editor;
pub(crate) mod calibration;
pub(super) mod controller_diagram;
mod gamepad_actions;
mod joypad;
mod profiles;
mod shortcuts;
pub(super) mod tilt;
mod wonderswan;
use super::search::{self, SettingId};
use super::{InputDevicesPage, SettingsUiState};
use crate::debug::DebugWindowState;
use crate::emu_backend::ActiveSystem;
use crate::settings::{GamepadAssignment, InputScope, InputSystem, Settings};
use controller_diagram::DiagramKind;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum BindingSource {
    #[default]
    Controller,
    Keyboard,
}

pub(super) fn scope_label(scope: &InputScope) -> String {
    match scope {
        InputScope::Global => "Global".into(),
        InputScope::System(system) => format!("System · {}", system.label()),
        InputScope::Game(_) => "This game".into(),
    }
}

fn draw_scope(ui: &mut egui::Ui, state: &mut DebugWindowState) {
    let previous = state.settings_ui.input_scope.clone();
    if let InputScope::Game(game) = &state.settings_ui.input_scope
        && state.settings_ui.current_input_game.as_ref() != Some(game)
    {
        state.settings_ui.input_scope = InputScope::System(game.system);
    }
    super::layout::row(ui, SettingId::InputDevicesMappingScope, None, |ui| {
        egui::ComboBox::from_id_salt("mapping_scope")
            .width(240.0)
            .selected_text(scope_label(&state.settings_ui.input_scope))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut state.settings_ui.input_scope,
                    InputScope::Global,
                    "Global",
                );
                for system in InputSystem::ALL {
                    ui.selectable_value(
                        &mut state.settings_ui.input_scope,
                        InputScope::System(system),
                        format!("System · {}", system.label()),
                    );
                }
                if let Some(game) = &state.settings_ui.current_input_game {
                    ui.selectable_value(
                        &mut state.settings_ui.input_scope,
                        InputScope::Game(game.clone()),
                        "This game",
                    );
                } else {
                    ui.add_enabled(false, egui::Button::new("This game · load a game first"));
                }
            })
            .response
    });
    let explanation = match &state.settings_ui.input_scope {
        InputScope::Global => {
            "Base mappings for all games. System and game overrides take priority.".to_owned()
        }
        InputScope::System(system) => format!(
            "Overrides for {}. Unchanged controls inherit Global mappings.",
            system.label()
        ),
        InputScope::Game(_) => format!(
            "Overrides for {}. Unchanged controls inherit System, then Global mappings.",
            state
                .settings_ui
                .current_input_game_name
                .as_deref()
                .unwrap_or("the loaded game")
        ),
    };
    super::layout::helper(ui, explanation);
    if previous != state.settings_ui.input_scope {
        cancel_capture(state);
        state.settings_ui.player_reset_confirmation = None;
        state.settings_ui.reset_confirmation = None;
        state.settings_ui.profile_action_confirmation = None;
    }
}

pub(super) fn prepare_search_target(state: &mut DebugWindowState, id: SettingId) {
    match id {
        SettingId::InputDevicesKeyboardMappings => {
            state.settings_ui.binding_source = BindingSource::Keyboard
        }
        SettingId::InputDevicesControllerMappings
        | SettingId::InputDevicesClearControllerMapping
        | SettingId::InputDevicesClearWonderswanDirectMappings => {
            state.settings_ui.binding_source = BindingSource::Controller
        }
        _ => {}
    }
    let title = search::metadata(id).title;
    if title.starts_with("WonderSwan ")
        || id == SettingId::InputDevicesClearWonderswanDirectMappings
    {
        state.settings_ui.controller_layout = Some(DiagramKind::WonderSwan);
        state.settings_ui.selected_player = 1;
    } else if title.ends_with(" mapping") || id == SettingId::InputDevicesClearControllerMapping {
        state.settings_ui.controller_layout = Some(DiagramKind::StandardGamepad);
    }
    if matches!(
        id,
        SettingId::InputDevicesUpdateSelectedProfile
            | SettingId::InputDevicesResetSelectedProfile
            | SettingId::InputDevicesDeleteSelectedProfile
            | SettingId::InputDevicesRenameSelectedProfile
            | SettingId::InputDevicesLoadProfileShortcuts
    ) {
        state.settings_ui.profile_manage_open = true;
    }
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
            super::layout::tab(ui, &mut state.settings_ui.input_page, page, page.label());
        }
    });
    if previous_page != state.settings_ui.input_page {
        cancel_capture(state);
    }
    ui.add_space(4.0);
    if state.settings_ui.input_page != InputDevicesPage::Hotkeys {
        draw_scope(ui, state);
        ui.add_space(4.0);
    }
    draw_capture_status(ui, state);
    match state.settings_ui.input_page {
        InputDevicesPage::Controls => draw_controller_workspace(ui, settings, state, active_system),
        InputDevicesPage::Hotkeys => {
            ui.label("Emulator shortcuts apply globally across all games and systems.");
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
    let state = &mut state.settings_ui;
    binding_editor::draw(
        ui.ctx(),
        settings,
        &mut state.binding_editor,
        &state.gamepad_snapshot,
        &mut state.gamepad_commands,
        &state.binding_keyboard_down,
        state.current_input_game.as_ref(),
    );
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
    super::layout::row(ui, SettingId::InputDevicesPlayer, None, |ui| {
        egui::ComboBox::from_id_salt("controls_player")
            .width(240.0)
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
                    if ui
                        .selectable_value(
                            &mut state.settings_ui.selected_player,
                            player,
                            format!("Player {player}"),
                        )
                        .changed()
                    {
                        joypad::clear_capture(state);
                    }
                }
            })
            .response
    });
    reconcile_layout(
        state,
        state
            .settings_ui
            .controller_layout
            .unwrap_or(automatic_layout),
    );
    ui.add_space(4.0);
    draw_player_connection(ui, settings, &mut state.settings_ui);
    ui.add_space(6.0);
    ui.strong("Mappings");
    super::layout::row(ui, SettingId::InputDevicesButtonLayout, None, |ui| {
        egui::ComboBox::from_id_salt("controls_layout")
            .width(240.0)
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
            })
            .response
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
    ui.horizontal_wrapped(|ui| {
        for (source, label) in [
            (BindingSource::Controller, "Controller"),
            (BindingSource::Keyboard, "Keyboard"),
        ] {
            let response =
                super::layout::tab(ui, &mut state.settings_ui.binding_source, source, label);
            search::target(
                ui,
                match source {
                    BindingSource::Keyboard => SettingId::InputDevicesKeyboardMappings,
                    BindingSource::Controller => SettingId::InputDevicesControllerMappings,
                },
                &response,
            );
            if response.changed() {
                joypad::clear_capture(state);
            }
        }
        ui.label(
            egui::RichText::new("Select a button or mapping to change it.")
                .small()
                .weak(),
        );
    });
    ui.add_space(4.0);
    let player = state.settings_ui.selected_player;
    let source = state.settings_ui.binding_source;
    let side_by_side_width = if state.settings_ui.input_scope == InputScope::Global {
        800.0
    } else {
        860.0
    };
    if ui.available_width() >= side_by_side_width {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(360.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_max_width(360.0);
                    draw_controller_reference(ui, settings, state, kind);
                },
            );
            ui.add_space(10.0);
            ui.vertical(|ui| {
                draw_mapping_table(ui, settings, state, player, source, kind);
            });
        });
    } else {
        egui::CollapsingHeader::new("Controller diagram")
            .id_salt("input_controller_diagram")
            .default_open(false)
            .show(ui, |ui| {
                draw_controller_reference(ui, settings, state, kind)
            });
        ui.add_space(4.0);
        draw_mapping_table(ui, settings, state, player, source, kind);
    }
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(
            "Diagram changes the visual guide. Physical device assignments apply globally.",
        )
        .weak(),
    );
    if player >= 3 {
        ui.label("Additional players depend on the game and connected controller ports.");
    }
    autofire::draw(ui, settings, &state.settings_ui.input_scope, player, kind);
    ui.add_space(8.0);
    let mut profile_action = false;
    let reveal_profiles = [
        SettingId::InputDevicesApplyInputProfile,
        SettingId::InputDevicesSaveCurrentAsProfile,
        SettingId::InputDevicesUpdateSelectedProfile,
        SettingId::InputDevicesResetSelectedProfile,
        SettingId::InputDevicesDeleteSelectedProfile,
        SettingId::InputDevicesRenameSelectedProfile,
        SettingId::InputDevicesLoadProfileShortcuts,
    ]
    .iter()
    .any(|&id| search::requested(ui, id));
    egui::CollapsingHeader::new(format!(
        "Controller profiles ({})",
        settings.input_profiles.profiles.len()
    ))
    .id_salt("saved_input_profiles")
    .open(reveal_profiles.then_some(true))
    .show(ui, |ui| {
        profile_action = profiles::draw(ui, settings, &mut state.settings_ui)
    });
    if profile_action {
        cancel_capture(state);
    }
}

fn draw_controller_reference(
    ui: &mut egui::Ui,
    settings: &Settings,
    state: &mut DebugWindowState,
    kind: DiagramKind,
) {
    let player = state.settings_ui.selected_player;
    let selected = joypad::captured_action(state, player);
    let pressed = state.settings_ui.gamepad_snapshot.players[usize::from(player - 1)].buttons;
    let interaction = controller_diagram::draw(
        ui,
        kind,
        pressed,
        selected,
        state.settings_ui.highlighted_mapping,
    );
    state.settings_ui.highlighted_mapping = interaction.highlighted;
    if let Some(action) = interaction.activated {
        use crate::settings::BindingTarget;
        use controller_diagram::DiagramAction;
        let (target, label) = match action {
            DiagramAction::Joypad(button) => (
                BindingTarget::Joypad {
                    player,
                    action: button,
                },
                kind.action_label(action),
            ),
            DiagramAction::WonderSwan(button) => {
                (BindingTarget::WonderSwan(button), button.label())
            }
        };
        let source = joypad::gameplay_source(state.settings_ui.binding_source);
        joypad::open_binding_editor(settings, state, target, source, label.to_owned());
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
        let response = ui.strong("Direct WonderSwan mappings");
        search::target(
            ui,
            SettingId::InputDevicesWonderswanDirectMappings,
            &response,
        );
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
            ui.label("Physical device");
            let device_response = egui::ComboBox::from_id_salt("player_device")
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
                }).response;
            search::target(ui, SettingId::InputDevicesInputDevice, &device_response);
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
fn draw_capture_status(ui: &mut egui::Ui, state: &mut DebugWindowState) {
    let capturing = state.settings_ui.captures_gameplay_input()
        || state.rebinding_action.is_some()
        || state.rebinding_shortcut.is_some()
        || state.rebinding_gamepad.is_some()
        || state.rebinding_gamepad_p2.is_some()
        || state.rebinding_gamepad_pce_multitap.is_some()
        || state.rebinding_ws_gamepad.is_some()
        || state.rebinding_gamepad_action.is_some()
        || state.rebinding_speedup
        || state.rebinding_rewind;

    if !capturing && search::requested(ui, SettingId::InputDevicesCancelCapture) {
        let response = ui.label("No capture is active.");
        search::target(ui, SettingId::InputDevicesCancelCapture, &response);
    }
    if capturing {
        ui.separator();

        ui.group(|ui| {
            let message = if state.settings_ui.calibration_capture.is_some() {
                "Calibration is active. Follow the steps below."
            } else if state.settings_ui.gamepad_snapshot.capture_active
                && !state.settings_ui.gamepad_snapshot.capture_ready
            {
                "Release held controller inputs before capture can begin…"
            } else {
                "Waiting for a control input…"
            };

            ui.label(egui::RichText::new(message).color(egui::Color32::YELLOW));

            let response = ui.button("Cancel capture");
            search::target(ui, SettingId::InputDevicesCancelCapture, &response);
            if response.clicked() {
                cancel_capture(state);
            }
        });
    }
}

fn draw_test_and_calibrate(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_ui: &mut SettingsUiState,
) {
    ui.label("Move either stick or press a button to inspect live input. Stick values are normalized by the input backend, before gameplay transforms.");
    ui.add_space(8.0);
    let devices = &settings_ui.gamepad_snapshot.devices;
    if !devices
        .iter()
        .any(|device| Some(device.id) == settings_ui.test_device)
    {
        settings_ui.test_device = devices.first().map(|device| device.id);
    }
    if devices.is_empty() {
        let response = ui
            .group(|ui| {
                ui.label("Connect a controller to test its buttons and sticks.");
            })
            .response;
        for id in [
            SettingId::InputDevicesTestControllerButtons,
            SettingId::InputDevicesTestLeftStick,
            SettingId::InputDevicesTestRightStick,
        ] {
            search::target(ui, id, &response);
        }
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
    let selected_device = devices
        .iter()
        .find(|device| Some(device.id) == settings_ui.test_device)
        .cloned();
    if let Some(device) = &selected_device {
        egui::Frame::group(ui.style())
            .inner_margin(14.0)
            .show(ui, |ui| {
                let deadzone = settings
                    .resolve_gameplay_input(&settings_ui.input_scope)
                    .tilt
                    .deadzone;
                let columns = ((ui.available_width() + 12.0) / 184.0).floor().clamp(1.0, 4.0) as usize;
                egui::Grid::new("stick_diagnostics")
                    .num_columns(columns)
                    .spacing([12.0, 12.0])
                    .show(ui, |ui| {
                        for (index, (title, value, threshold, target)) in [
                            ("Left stick · raw input", device.left_stick, Some(deadzone), SettingId::InputDevicesTestLeftStick),
                            ("Left stick · after calibration", device.calibrated_left_stick, Some(deadzone), SettingId::InputDevicesTestLeftStick),
                            ("Right stick · raw input", device.right_stick, None, SettingId::InputDevicesTestRightStick),
                            ("Right stick · after calibration", device.calibrated_right_stick, None, SettingId::InputDevicesTestRightStick),
                        ].into_iter().enumerate() {
                            let response = stick_plot(ui, title, value, threshold);
                            search::target(ui, target, &response);
                            if (index + 1) % columns == 0 { ui.end_row(); }
                        }
                    });
                let output = crate::input::transforms::stick_dpad_vector(
                    device.calibrated_left_stick,
                    deadzone,
                );
                ui.strong("After Zeff transform · D-pad preview");
                ui.label(
                    egui::RichText::new(format!(
                        "Raw ({:+.2}, {:+.2}) → Calibrated ({:+.2}, {:+.2}) → Effective ({:+.0}, {:+.0})",
                        device.left_stick.0,
                        device.left_stick.1,
                        device.calibrated_left_stick.0,
                        device.calibrated_left_stick.1,
                        output.0,
                        output.1,
                    ))
                    .monospace(),
                );
                ui.label(egui::RichText::new("Calibration precedes the selected-scope deadzone and cardinal snap. Either stick can also drive an explicit axis binding, with its own curve and thresholds.").weak());
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    let response = ui.strong(format!("Pressed: {}", device.buttons.len()));
                    search::target(ui, SettingId::InputDevicesTestControllerButtons, &response);
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
    calibration::draw(ui, settings, settings_ui, selected_device.as_ref());
    ui.add_space(8.0);
    tilt::transform_row(
        ui,
        settings,
        &settings_ui.input_scope,
        crate::settings::GameplayTransform::Deadzone,
        SettingId::InputDevicesLeftStickDeadzone,
    );
    ui.label(egui::RichText::new("The shaded ring shows the selected scope’s implicit left-stick threshold. Explicit axis bindings have their own thresholds and curves.").weak());
    ui.add_space(8.0);
    egui::CollapsingHeader::new("Mapped state · live gameplay & player assignments").default_open(true).show(ui, |ui| {
        ui.add(egui::Label::new("Controller buttons below reflect the loaded game’s active mappings and implicit stick directions, before per-frame autofire. Keyboard and remote input are separate sources.").wrap());
        let ws: Vec<_> = crate::settings::WonderSwanButton::ALL.iter().enumerate()
            .filter(|(index, _)| settings_ui.gamepad_snapshot.wonderswan_buttons & (1 << index) != 0)
            .map(|(_, button)| button.label()).collect();
        if !ws.is_empty() {
            ui.label(format!("Direct WonderSwan controls: {}", ws.join(", ")));
        }
        if ui.available_width() < 640.0 {
            for (index, player) in settings_ui.gamepad_snapshot.players.iter().enumerate() {
                ui.group(|ui| {
                    ui.set_max_width(ui.available_width());
                    ui.strong(format!("Player {}", index + 1));
                    ui.add(egui::Label::new(player_status_label(player.status)).wrap());
                    ui.add(egui::Label::new(mapped_button_labels(player.buttons)).wrap());
                });
            }
            return;
        }
        if ui.available_width() < 620.0 {
            for (index, player) in settings_ui.gamepad_snapshot.players.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(format!("Player {}", index + 1));
                    ui.label(player_status_label(player.status));
                });
                ui.label(mapped_button_labels(player.buttons));
                ui.add_space(4.0);
            }
        } else {
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
        }
    });
    draw_input_timing(ui, &mut settings_ui.input_timing);
}

fn draw_input_timing(ui: &mut egui::Ui, timing: &mut crate::input::timing::InputTiming) {
    egui::CollapsingHeader::new("Input timing measurements").show(ui, |ui| {
        ui.label("Internal input polling and rendering timings. These do not measure controller-to-screen latency; presentation timestamps are unavailable.");
        ui.horizontal_wrapped(|ui| {
            let mut enabled = timing.enabled();
            if ui.checkbox(&mut enabled, "Record measurements").changed() {
                timing.set_enabled(enabled);
            }
            if ui.button("Clear measurements").clicked() {
                timing.reset();
            }
            if ui.button("Copy report").clicked() {
                ui.ctx().copy_text(timing.report());
            }
        });
        let summary = timing.summary();
        for (label, distribution) in [
            ("Poll interval", summary.poll_interval),
            ("Event observed to snapshot", summary.event_to_snapshot),
            ("Snapshot to Settings frame", summary.snapshot_to_frame),
            ("Settings frame to CPU submission", summary.frame_to_submission),
            ("Event observed to CPU submission", summary.event_to_submission),
        ] {
            ui.strong(label);
            if distribution.samples == 0 {
                ui.weak("No samples yet");
            } else {
                ui.label(format!(
                    "p50 {:.2} · p95 {:.2} · p99 {:.2} · max {:.2} ms · {} samples",
                    distribution.p50_ms, distribution.p95_ms, distribution.p99_ms,
                    distribution.max_ms, distribution.samples,
                ));
            }
        }
        ui.label(format!(
            "{} events · {} polls · {} submitted frames",
            summary.observed_events, summary.polls, summary.submitted_frames,
        ));
        ui.label(format!(
            "{} coalesced events · {} redraws without new events · {} invalid samples",
            summary.coalesced_events, summary.duplicate_frames, summary.invalid_samples,
        ));
        ui.weak("Coalesced events occurred between submitted snapshots. Physical missed transitions require independent hardware measurement. Distributions keep the latest 256 samples; counters cover this recording session.");
    });
}

fn stick_plot(
    ui: &mut egui::Ui,
    title: &str,
    value: (f32, f32),
    deadzone: Option<f32>,
) -> egui::Response {
    ui.vertical(|ui| {
        ui.set_width(172.0);
        ui.allocate_ui(egui::vec2(172.0, 44.0), |ui| {
            ui.set_min_height(44.0);
            ui.label(egui::RichText::new(title).strong());
        });
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
    })
    .response
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
    state.settings_ui.cancel_input_capture();
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

use super::BindingSource;
use super::controller_diagram::{DiagramAction, DiagramKind};
use crate::debug::DebugWindowState;
use crate::settings::{BindingAction, InputBindingAction, Settings};
use winit::keyboard::KeyCode;

pub(super) fn draw_focused(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    player: u8,
    source: BindingSource,
    kind: DiagramKind,
) {
    egui::Grid::new(("focused_joypad_bindings", player))
        .num_columns(3)
        .spacing([12.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Control");
            ui.strong(match source {
                BindingSource::Keyboard => "Keyboard key",
                BindingSource::Controller => "Controller button",
            });
            ui.strong("");
            ui.end_row();
            for action in kind.actions() {
                let DiagramAction::Joypad(action) = action else {
                    continue;
                };
                ui.label(kind.action_label(DiagramAction::Joypad(action)));
                let conflict = match source {
                    BindingSource::Keyboard => {
                        draw_keyboard_binding(ui, settings, state, player, action);
                        keyboard_conflict(settings, player, action)
                    }
                    BindingSource::Controller => {
                        draw_gamepad_binding(ui, settings, state, player, action);
                        gamepad_conflict(settings, player, action)
                    }
                };
                if let Some(other) = conflict {
                    ui.label(egui::RichText::new("!").color(egui::Color32::from_rgb(240, 180, 70)))
                        .on_hover_text(format!(
                            "Also assigned to {}. Existing first-match priority applies.",
                            joypad_label(other)
                        ));
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    ui.add_space(8.0);
    if ui.button("Restore default mappings…").clicked() {
        state.settings_ui.player_reset_confirmation = Some(match source {
            BindingSource::Keyboard => super::super::PlayerResetTarget::Keyboard(player),
            BindingSource::Controller => super::super::PlayerResetTarget::Gamepad(player),
        });
    }
    draw_player_reset_confirmation(ui, settings, state);
}

pub(super) fn begin_capture(
    state: &mut DebugWindowState,
    player: u8,
    source: BindingSource,
    action: DiagramAction,
) {
    clear_capture(state);
    match (source, action) {
        (BindingSource::Keyboard, DiagramAction::Joypad(action)) => {
            state.rebinding_action = Some(keyboard_capture_action(player, action))
        }
        (BindingSource::Keyboard, DiagramAction::WonderSwan(action)) => {
            state.rebinding_action = Some(InputBindingAction::WonderSwan(action))
        }
        (BindingSource::Controller, DiagramAction::Joypad(action)) => match player {
            1 => state.rebinding_gamepad = Some(action),
            2 => state.rebinding_gamepad_p2 = Some(action),
            _ => state.rebinding_gamepad_pce_multitap = Some((player, action)),
        },
        (BindingSource::Controller, DiagramAction::WonderSwan(action)) => {
            state.rebinding_ws_gamepad = Some(action)
        }
    }
}

pub(super) fn captured_action(state: &DebugWindowState, player: u8) -> Option<DiagramAction> {
    if let Some(action) = state.rebinding_action {
        return match action {
            InputBindingAction::Joypad(action)
            | InputBindingAction::JoypadP2(action)
            | InputBindingAction::PceMultitap { action, .. } => Some(DiagramAction::Joypad(action)),
            InputBindingAction::WonderSwan(action) => Some(DiagramAction::WonderSwan(action)),
            InputBindingAction::Tilt(_) => None,
        };
    }
    if let Some(action) = state.rebinding_ws_gamepad {
        return Some(DiagramAction::WonderSwan(action));
    }
    match player {
        1 => state.rebinding_gamepad,
        2 => state.rebinding_gamepad_p2,
        _ => state
            .rebinding_gamepad_pce_multitap
            .map(|(_, action)| action),
    }
    .map(DiagramAction::Joypad)
}

pub(super) fn key_label(key: KeyCode) -> String {
    let name = format!("{key:?}");
    match key {
        KeyCode::ArrowUp => "Up".into(),
        KeyCode::ArrowDown => "Down".into(),
        KeyCode::ArrowLeft => "Left".into(),
        KeyCode::ArrowRight => "Right".into(),
        KeyCode::ShiftLeft => "Left Shift".into(),
        KeyCode::ShiftRight => "Right Shift".into(),
        KeyCode::ControlLeft => "Left Ctrl".into(),
        KeyCode::ControlRight => "Right Ctrl".into(),
        KeyCode::AltLeft => "Left Alt".into(),
        KeyCode::AltRight => "Right Alt".into(),
        KeyCode::BracketLeft => "[".into(),
        KeyCode::BracketRight => "]".into(),
        KeyCode::Backquote => "`".into(),
        KeyCode::Backslash => "\\".into(),
        KeyCode::Comma => ",".into(),
        KeyCode::Period => ".".into(),
        KeyCode::Slash => "/".into(),
        KeyCode::Semicolon => ";".into(),
        KeyCode::Quote => "'".into(),
        KeyCode::Minus => "-".into(),
        KeyCode::Equal => "=".into(),
        _ => name
            .strip_prefix("Key")
            .or_else(|| name.strip_prefix("Digit"))
            .map_or_else(|| name.clone(), str::to_owned),
    }
}

pub(super) fn gamepad_label(button: &str) -> String {
    match button {
        "" => "Unbound".into(),
        "South" => "South (bottom)".into(),
        "East" => "East (right)".into(),
        "West" => "West (left)".into(),
        "North" => "North (top)".into(),
        "DPadUp" => "D-pad Up".into(),
        "DPadDown" => "D-pad Down".into(),
        "DPadLeft" => "D-pad Left".into(),
        "DPadRight" => "D-pad Right".into(),
        "LeftTrigger" => "Left shoulder".into(),
        "RightTrigger" => "Right shoulder".into(),
        "LeftTrigger2" => "Left trigger".into(),
        "RightTrigger2" => "Right trigger".into(),
        _ => button.to_owned(),
    }
}

pub(super) fn draw_player_reset_confirmation(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
) {
    let Some(target) = state.settings_ui.player_reset_confirmation else {
        return;
    };
    ui.group(|ui| {
        let (player, kind) = match target {
            super::super::PlayerResetTarget::Keyboard(player) => (player, "keyboard"),
            super::super::PlayerResetTarget::Gamepad(player) => (player, "gamepad"),
            super::super::PlayerResetTarget::WonderSwanKeyboard => (1, "WonderSwan keyboard"),
            super::super::PlayerResetTarget::WonderSwanGamepad => (1, "WonderSwan controller"),
            super::super::PlayerResetTarget::WonderSwanClear => (1, "direct WonderSwan controller"),
        };
        let clearing = matches!(target, super::super::PlayerResetTarget::WonderSwanClear);
        ui.label(if clearing {
            "Clear all direct WonderSwan controller mappings?".to_owned()
        } else {
            format!("Reset Player {player} {kind} mappings?")
        });
        ui.horizontal(|ui| {
            if ui
                .button(if clearing { "Clear" } else { "Reset" })
                .clicked()
            {
                let previous = settings.clone();
                match target {
                    super::super::PlayerResetTarget::Keyboard(player) => {
                        reset_keyboard_player(settings, player)
                    }
                    super::super::PlayerResetTarget::Gamepad(player) => {
                        reset_gamepad_player(settings, player)
                    }
                    super::super::PlayerResetTarget::WonderSwanKeyboard => {
                        settings.ws_key_bindings =
                            crate::settings::WonderSwanKeyBindings::default();
                    }
                    super::super::PlayerResetTarget::WonderSwanGamepad => {
                        settings.gamepad_bindings.reset_wonderswan_defaults();
                    }
                    super::super::PlayerResetTarget::WonderSwanClear => {
                        settings.gamepad_bindings.clear_wonderswan_direct_bindings();
                    }
                }
                clear_capture(state);
                state.settings_ui.undo = Some(previous);
                state.settings_ui.undo_baseline = Some(settings.clone());
                state.settings_ui.undo_label = Some("Player mappings reset.".to_string());
                state.settings_ui.player_reset_confirmation = None;
            }
            if ui.button("Cancel").clicked() {
                state.settings_ui.player_reset_confirmation = None;
            }
        });
    });
}

fn draw_keyboard_binding(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    player: u8,
    action: BindingAction,
) {
    let key = keyboard_binding(settings, player, action);
    let key_name = key.map_or_else(|| "Unbound".to_owned(), key_label);
    let capturing = state.rebinding_action == Some(keyboard_capture_action(player, action));
    let label = if capturing {
        format!("Press key… ({key_name})")
    } else {
        key_name
    };
    if ui
        .add_sized([170.0, 26.0], egui::Button::new(label))
        .clicked()
    {
        begin_capture(
            state,
            player,
            BindingSource::Keyboard,
            DiagramAction::Joypad(action),
        );
    }
}

fn draw_gamepad_binding(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    player: u8,
    action: BindingAction,
) {
    let bound = settings.gamepad_bindings.get_for_player(action, player);
    let display = gamepad_label(bound);
    let capturing = match player {
        1 => state.rebinding_gamepad == Some(action),
        2 => state.rebinding_gamepad_p2 == Some(action),
        _ => state.rebinding_gamepad_pce_multitap == Some((player, action)),
    };
    let label = if capturing {
        format!("Press button… ({display})")
    } else {
        display.to_owned()
    };
    if ui
        .add_sized([170.0, 26.0], egui::Button::new(label))
        .clicked()
    {
        begin_capture(
            state,
            player,
            BindingSource::Controller,
            DiagramAction::Joypad(action),
        );
    }
}

fn keyboard_binding(settings: &Settings, player: u8, action: BindingAction) -> Option<KeyCode> {
    match player {
        1 => Some(settings.key_bindings.get(action)),
        2 => Some(settings.key_bindings_p2.get(action)),
        3..=5 => settings.pce_multitap_key_bindings[usize::from(player - 3)].get(action),
        _ => Some(settings.key_bindings.get(action)),
    }
}
fn keyboard_capture_action(player: u8, action: BindingAction) -> InputBindingAction {
    match player {
        1 => InputBindingAction::Joypad(action),
        2 => InputBindingAction::JoypadP2(action),
        _ => InputBindingAction::PceMultitap { player, action },
    }
}

fn keyboard_conflict(
    settings: &Settings,
    player: u8,
    current: BindingAction,
) -> Option<BindingAction> {
    let key = keyboard_binding(settings, player, current)?;
    BindingAction::ALL.iter().copied().find(|&action| {
        keyboard_binding(settings, player, action) == Some(key) && action != current
    })
}

fn gamepad_conflict(
    settings: &Settings,
    player: u8,
    current: BindingAction,
) -> Option<BindingAction> {
    let button = settings.gamepad_bindings.get_for_player(current, player);
    if button.is_empty() {
        return None;
    }
    BindingAction::ALL.iter().copied().find(|&action| {
        action != current && settings.gamepad_bindings.get_for_player(action, player) == button
    })
}

fn reset_keyboard_player(settings: &mut Settings, player: u8) {
    match player {
        1 => settings.key_bindings = crate::settings::KeyBindings::default(),
        2 => settings.key_bindings_p2 = crate::settings::KeyBindings::player_two_defaults(),
        3..=5 => {
            settings.pce_multitap_key_bindings[usize::from(player - 3)] =
                crate::settings::PceMultitapKeyBindings::default();
        }
        _ => {}
    }
}

fn reset_gamepad_player(settings: &mut Settings, player: u8) {
    let defaults = crate::settings::GamepadBindings::default();
    for &action in BindingAction::ALL {
        let value = defaults.get_for_player(action, player);
        settings
            .gamepad_bindings
            .set_for_player(action, player, value);
    }
}

pub(super) fn clear_capture(state: &mut DebugWindowState) {
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

fn joypad_label(action: BindingAction) -> &'static str {
    match action {
        BindingAction::Up => "Up",
        BindingAction::Down => "Down",
        BindingAction::Left => "Left",
        BindingAction::Right => "Right",
        BindingAction::A => "A",
        BindingAction::B => "B",
        BindingAction::X => "X",
        BindingAction::Y => "Y",
        BindingAction::L => "L",
        BindingAction::R => "R",
        BindingAction::Start => "Start",
        BindingAction::Select => "Select",
    }
}

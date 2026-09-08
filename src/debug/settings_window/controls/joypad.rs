use super::BindingSource;
use super::controller_diagram::{DiagramAction, DiagramKind};
use crate::debug::DebugWindowState;
use crate::debug::settings_window::search::{self, SettingId};
use crate::settings::{
    BindingAction, BindingSet, BindingTarget, GamepadAction, GameplayBindingSource,
    InputBindingAction, InputScope, PhysicalBinding, Settings, ShortcutAction,
};
use winit::keyboard::KeyCode;

const SHADED_MAPPING_WIDTH: f32 = 560.0;
pub(super) const MAPPING_LABEL_WIDTH: f32 = 76.0;
pub(super) const MAPPING_SOURCE_WIDTH: f32 = 60.0;
const MIN_BINDING_WIDTH: f32 = 140.0;
const BASE_ACTIONS_WIDTH: f32 = 88.0;
const INHERIT_ACTION_WIDTH: f32 = 120.0;
pub(super) const MAPPING_ROW_HEIGHT: f32 = 26.0;
pub(super) const MAPPING_SPACING: f32 = 4.0;

struct JoypadBindingRow<'a> {
    settings: &'a mut Settings,
    state: &'a mut DebugWindowState,
    target: BindingTarget,
    gameplay_source: GameplayBindingSource,
    kind: DiagramKind,
    action: BindingAction,
    player: u8,
    diagram_action: DiagramAction,
    value: Option<BindingSet>,
    origin: InputScope,
}

pub(super) fn draw_focused(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    player: u8,
    source: BindingSource,
    kind: DiagramKind,
) {
    draw_mapping_header(
        ui,
        source,
        state.settings_ui.input_scope != InputScope::Global,
    );

    for action in kind.actions() {
        let DiagramAction::Joypad(action) = action else {
            continue;
        };
        draw_binding_row(ui, settings, state, source, kind, action);
    }

    ui.add_space(8.0);
    let restore = ui.button("Restore mappings for this scope…");
    search::target(ui, SettingId::InputDevicesRestoreDefaultMappings, &restore);
    if restore.clicked() {
        state.settings_ui.player_reset_confirmation = Some(match source {
            BindingSource::Keyboard => super::super::PlayerResetTarget::Keyboard(player),
            BindingSource::Controller => super::super::PlayerResetTarget::Gamepad(player),
        });
    }
    draw_player_reset_confirmation(ui, settings, state);
}

pub(super) fn draw_mapping_header(ui: &mut egui::Ui, source: BindingSource, scoped: bool) {
    let binding_label = match source {
        BindingSource::Keyboard => "Keyboard binding",
        BindingSource::Controller => "Controller binding",
    };
    let binding_width = mapping_row_layout(ui, scoped).binding_width;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = MAPPING_SPACING;
        ui.add_sized(
            [MAPPING_LABEL_WIDTH, 20.0],
            egui::Label::new(egui::RichText::new("Control").strong()),
        );
        ui.add_sized(
            [binding_width, 20.0],
            egui::Label::new(egui::RichText::new(binding_label).strong()),
        );
        ui.add_sized(
            [MAPPING_SOURCE_WIDTH, 20.0],
            egui::Label::new(egui::RichText::new("Source").strong()),
        );
    });
}

fn draw_binding_row(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    source: BindingSource,
    kind: DiagramKind,
    action: BindingAction,
) {
    let player = state.settings_ui.selected_player;
    let diagram_action = DiagramAction::Joypad(action);
    let target = BindingTarget::Joypad { player, action };
    let gameplay_source = gameplay_source(source);
    let resolved = settings.binding_set(&state.settings_ui.input_scope, target, gameplay_source);
    let highlighted = state.settings_ui.highlighted_mapping == Some(diagram_action);
    let mut row = JoypadBindingRow {
        settings,
        state,
        target,
        gameplay_source,
        kind,
        action,
        player,
        diagram_action,
        value: resolved.value,
        origin: resolved.origin,
    };
    let available_width = ui.available_width();
    let layout = mapping_row_layout(
        ui,
        row.state.settings_ui.input_scope != InputScope::Global
            && row.origin == row.state.settings_ui.input_scope,
    );
    let shaded = available_width < SHADED_MAPPING_WIDTH;
    let frame = if highlighted || shaded {
        egui::Frame::NONE.fill(if highlighted {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().faint_bg_color
        })
    } else {
        egui::Frame::NONE
    };
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = MAPPING_SPACING;
        if layout.stacked {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [MAPPING_LABEL_WIDTH, MAPPING_ROW_HEIGHT],
                    egui::Label::new(row.kind.action_label(row.diagram_action)),
                );
                draw_binding_button(ui, layout.binding_width, &mut row);
                ui.add_sized(
                    [MAPPING_SOURCE_WIDTH, MAPPING_ROW_HEIGHT],
                    egui::Label::new(origin_label(&row.origin)).sense(egui::Sense::hover()),
                )
                .on_hover_text(format!(
                    "This value comes from the {} scope.",
                    origin_label(&row.origin)
                ));
            });
            ui.horizontal(|ui| {
                ui.add_space(MAPPING_LABEL_WIDTH);
                draw_binding_actions(ui, &mut row);
            });
        } else {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [MAPPING_LABEL_WIDTH, MAPPING_ROW_HEIGHT],
                    egui::Label::new(row.kind.action_label(row.diagram_action)),
                );
                draw_binding_button(ui, layout.binding_width, &mut row);
                ui.add_sized(
                    [MAPPING_SOURCE_WIDTH, MAPPING_ROW_HEIGHT],
                    egui::Label::new(origin_label(&row.origin)).sense(egui::Sense::hover()),
                )
                .on_hover_text(format!(
                    "This value comes from the {} scope.",
                    origin_label(&row.origin)
                ));
                draw_binding_actions(ui, &mut row);
            });
        }
    });
}

#[derive(Clone, Copy)]
pub(super) struct MappingRowLayout {
    pub(super) binding_width: f32,
    pub(super) stacked: bool,
}

pub(super) fn mapping_row_layout(ui: &egui::Ui, has_inherit: bool) -> MappingRowLayout {
    let actions_width = BASE_ACTIONS_WIDTH
        + if has_inherit {
            INHERIT_ACTION_WIDTH
        } else {
            0.0
        };
    let binding_width = header_binding_width(ui);
    MappingRowLayout {
        binding_width,
        stacked: ui.available_width()
            < MAPPING_LABEL_WIDTH
                + MAPPING_SOURCE_WIDTH
                + binding_width
                + actions_width
                + 3.0 * MAPPING_SPACING,
    }
}

fn header_binding_width(ui: &egui::Ui) -> f32 {
    (ui.available_width()
        - MAPPING_LABEL_WIDTH
        - MAPPING_SOURCE_WIDTH
        - BASE_ACTIONS_WIDTH
        - 3.0 * MAPPING_SPACING)
        .clamp(MIN_BINDING_WIDTH, 170.0)
}

fn draw_binding_button(ui: &mut egui::Ui, width: f32, row: &mut JoypadBindingRow<'_>) {
    let label = binding_set_label(row.value.as_ref());
    let binding = ui
        .add_sized(
            [width, MAPPING_ROW_HEIGHT],
            egui::Button::new(&label).truncate(),
        )
        .on_hover_text(format!("{label}\nOpen the binding editor"));
    search::target(ui, mapping_search_id(row.action), &binding);
    if binding.hovered() || binding.has_focus() {
        row.state.settings_ui.highlighted_mapping = Some(row.diagram_action);
    }
    if binding.clicked() {
        open_binding_editor(
            row.settings,
            row.state,
            row.target,
            row.gameplay_source,
            row.kind.action_label(row.diagram_action).to_owned(),
        );
    }
}

fn draw_binding_actions(ui: &mut egui::Ui, row: &mut JoypadBindingRow<'_>) {
    let gameplay_conflict = binding_conflict(
        row.settings,
        &row.state.settings_ui.input_scope,
        row.player,
        row.action,
        row.gameplay_source,
    );
    let hotkey_conflicts = global_hotkey_conflicts_set(row.settings, row.value.as_ref());
    if gameplay_conflict.is_some() || !hotkey_conflicts.is_empty() {
        let mut message = gameplay_conflict
            .map(|other| {
                format!(
                    "Also assigned to {}. Existing first-match priority applies.",
                    row.kind.action_label(DiagramAction::Joypad(other))
                )
            })
            .unwrap_or_default();
        if !hotkey_conflicts.is_empty() {
            if !message.is_empty() {
                message.push(' ');
            }
            message.push_str(&format!(
                "Also assigned to global {}.",
                hotkey_conflicts.join(", ")
            ));
        }
        ui.label(egui::RichText::new("!").color(egui::Color32::from_rgb(240, 180, 70)))
            .on_hover_text(message);
    } else {
        ui.add_space(10.0);
    }
    let unbind = ui.small_button("Unbind");
    search::target(ui, SettingId::InputDevicesClearControllerMapping, &unbind);
    if unbind.clicked() {
        edit_binding(
            row.settings,
            row.state,
            row.target,
            row.gameplay_source,
            false,
        );
    }
    if !matches!(row.state.settings_ui.input_scope, InputScope::Global)
        && row.origin == row.state.settings_ui.input_scope
    {
        let inherited = row.origin != row.state.settings_ui.input_scope;
        if ui
            .add_enabled(!inherited, egui::Button::new("Use inherited"))
            .clicked()
        {
            edit_binding(
                row.settings,
                row.state,
                row.target,
                row.gameplay_source,
                true,
            );
        }
    }
}

#[cfg(test)]
pub(super) fn begin_capture(
    state: &mut DebugWindowState,
    player: u8,
    source: BindingSource,
    action: DiagramAction,
) {
    clear_capture(state);
    state.settings_ui.input_capture_scope = Some(state.settings_ui.input_scope.clone());
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
    if let Some(editor) = &state.settings_ui.binding_editor {
        match editor.target() {
            BindingTarget::Joypad {
                player: target_player,
                action,
            } if target_player == player => return Some(DiagramAction::Joypad(action)),
            BindingTarget::WonderSwan(action) if player == 1 => {
                return Some(DiagramAction::WonderSwan(action));
            }
            _ => {}
        }
    }
    if let Some(action) = state.rebinding_action {
        return match action {
            InputBindingAction::Joypad(action)
            | InputBindingAction::JoypadP2(action)
            | InputBindingAction::PceMultitap { action, .. } => Some(DiagramAction::Joypad(action)),
            InputBindingAction::WonderSwan(action) => Some(DiagramAction::WonderSwan(action)),
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
            "Unbind all direct WonderSwan controller mappings in this scope?".to_owned()
        } else {
            format!("Restore Player {player} {kind} mappings for this scope?")
        });
        ui.horizontal(|ui| {
            if ui
                .button(if clearing { "Unbind all" } else { "Restore" })
                .clicked()
            {
                let previous = settings.clone();
                reset_target_for_scope(settings, &state.settings_ui.input_scope, target);
                clear_capture(state);
                state.settings_ui.undo = Some(previous);
                state.settings_ui.undo_baseline = Some(settings.clone());
                state.settings_ui.undo_label = Some("Mappings restored for this scope.".to_owned());
                state.settings_ui.player_reset_confirmation = None;
            }
            if ui.button("Cancel").clicked() {
                state.settings_ui.player_reset_confirmation = None;
            }
        });
    });
}

fn reset_target_for_scope(
    settings: &mut Settings,
    scope: &InputScope,
    reset: super::super::PlayerResetTarget,
) {
    let (player, source, wonder_swan, clear) = match reset {
        super::super::PlayerResetTarget::Keyboard(player) => {
            (player, GameplayBindingSource::Keyboard, false, false)
        }
        super::super::PlayerResetTarget::Gamepad(player) => {
            (player, GameplayBindingSource::Gamepad, false, false)
        }
        super::super::PlayerResetTarget::WonderSwanKeyboard => {
            (1, GameplayBindingSource::Keyboard, true, false)
        }
        super::super::PlayerResetTarget::WonderSwanGamepad => {
            (1, GameplayBindingSource::Gamepad, true, false)
        }
        super::super::PlayerResetTarget::WonderSwanClear => {
            (1, GameplayBindingSource::Gamepad, true, true)
        }
    };
    let targets = if wonder_swan {
        crate::settings::WonderSwanButton::ALL
            .iter()
            .copied()
            .map(BindingTarget::WonderSwan)
            .collect::<Vec<_>>()
    } else {
        BindingAction::ALL
            .iter()
            .copied()
            .map(|action| BindingTarget::Joypad { player, action })
            .collect::<Vec<_>>()
    };
    if clear {
        for target in targets {
            let _ = settings.set_binding(scope, target, source, None);
        }
    } else if matches!(scope, InputScope::Global) {
        let defaults = Settings::default();
        for target in targets {
            let value = defaults.binding(&InputScope::Global, target, source).value;
            let _ = settings.set_binding(scope, target, source, value);
        }
    } else {
        for target in targets {
            settings.inherit_binding(scope, target, source);
        }
    }
}

pub(super) fn edit_binding(
    settings: &mut Settings,
    state: &mut DebugWindowState,
    target: BindingTarget,
    source: GameplayBindingSource,
    inherit: bool,
) {
    let previous = settings.clone();
    if inherit {
        settings.inherit_binding(&state.settings_ui.input_scope, target, source);
    } else {
        let _ = settings.set_binding(&state.settings_ui.input_scope, target, source, None);
    }
    clear_capture(state);
    state.settings_ui.undo = Some(previous);
    state.settings_ui.undo_baseline = Some(settings.clone());
    state.settings_ui.undo_label = Some(if inherit {
        "Inherited mapping restored.".to_owned()
    } else {
        "Mapping unbound.".to_owned()
    });
}

pub(super) fn gameplay_source(source: BindingSource) -> GameplayBindingSource {
    match source {
        BindingSource::Keyboard => GameplayBindingSource::Keyboard,
        BindingSource::Controller => GameplayBindingSource::Gamepad,
    }
}

pub(super) fn origin_label(scope: &InputScope) -> &'static str {
    match scope {
        InputScope::Global => "Global",
        InputScope::System(_) => "System",
        InputScope::Game(_) => "Game",
    }
}

fn binding_conflict(
    settings: &Settings,
    scope: &InputScope,
    player: u8,
    current: BindingAction,
    source: GameplayBindingSource,
) -> Option<BindingAction> {
    let current_target = BindingTarget::Joypad {
        player,
        action: current,
    };
    let value = settings.binding_set(scope, current_target, source).value?;
    BindingAction::ALL.iter().copied().find(|&action| {
        action != current
            && settings
                .binding_set(scope, BindingTarget::Joypad { player, action }, source)
                .value
                .is_some_and(|other| binding_sets_overlap(&value, &other))
    })
}

pub(super) fn open_binding_editor(
    settings: &Settings,
    state: &mut DebugWindowState,
    target: BindingTarget,
    source: GameplayBindingSource,
    action_label: String,
) {
    clear_capture(state);
    let scope = state.settings_ui.input_scope.clone();
    let resolved = settings.binding_set(&scope, target, source);
    state.settings_ui.binding_editor = Some(super::binding_editor::BindingEditor::open(
        scope,
        target,
        source,
        action_label,
        resolved,
    ));
}

pub(super) fn binding_set_label(binding: Option<&BindingSet>) -> String {
    binding.map_or_else(|| "Unbound".into(), BindingSet::label)
}

pub(super) fn binding_sets_overlap(left: &BindingSet, right: &BindingSet) -> bool {
    left.alternatives.iter().any(|left| {
        right
            .alternatives
            .iter()
            .any(|right| left.expression == right.expression)
    })
}

pub(super) fn global_hotkey_conflicts_set(
    settings: &Settings,
    binding: Option<&BindingSet>,
) -> Vec<&'static str> {
    let mut conflicts = Vec::new();
    for alternative in binding.into_iter().flat_map(|set| &set.alternatives) {
        for key in alternative.expression.keyboard_keys() {
            conflicts.extend(global_hotkey_conflicts(
                settings,
                Some(&PhysicalBinding::Keyboard(key)),
            ));
        }
        collect_gamepad_conflicts(settings, &alternative.expression, &mut conflicts);
    }
    conflicts.sort_unstable();
    conflicts.dedup();
    conflicts
}

fn collect_gamepad_conflicts(
    settings: &Settings,
    expression: &crate::settings::BindingExpression,
    conflicts: &mut Vec<&'static str>,
) {
    match &expression.kind {
        crate::settings::BindingExpressionKind::GamepadButton(button) => {
            conflicts.extend(global_hotkey_conflicts(
                settings,
                Some(&PhysicalBinding::Gamepad(button.clone())),
            ));
        }
        crate::settings::BindingExpressionKind::Chord(atoms) => {
            for atom in atoms {
                collect_gamepad_conflicts(settings, atom, conflicts);
            }
        }
        _ => {}
    }
}

pub(super) fn global_hotkey_conflicts(
    settings: &Settings,
    binding: Option<&PhysicalBinding>,
) -> Vec<&'static str> {
    match binding {
        Some(PhysicalBinding::Keyboard(key)) => {
            let mut ids = vec![
                (settings.speedup_key_code() == *key).then_some(SettingId::InputDevicesSpeedUpKey),
                (settings.rewind.key_code() == *key).then_some(SettingId::InputDevicesRewindKey),
            ];
            ids.extend(ShortcutAction::ALL.iter().copied().map(|action| {
                (settings.shortcut_bindings.get(action) == *key)
                    .then_some(super::shortcuts::shortcut_id(action))
            }));
            ids.into_iter()
                .flatten()
                .map(|id| search::metadata(id).title)
                .collect()
        }
        Some(PhysicalBinding::Gamepad(button)) if !button.is_empty() => [
            GamepadAction::SpeedUp,
            GamepadAction::Rewind,
            GamepadAction::Pause,
            GamepadAction::Turbo,
        ]
        .into_iter()
        .filter(|&action| settings.gamepad_bindings.get_action(action) == button.as_str())
        .map(|action| search::metadata(super::gamepad_actions::action_id(action)).title)
        .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
fn keyboard_capture_action(player: u8, action: BindingAction) -> InputBindingAction {
    match player {
        1 => InputBindingAction::Joypad(action),
        2 => InputBindingAction::JoypadP2(action),
        _ => InputBindingAction::PceMultitap { player, action },
    }
}

fn mapping_search_id(action: BindingAction) -> SettingId {
    match action {
        BindingAction::Up => SettingId::InputDevicesDPadUpMapping,
        BindingAction::Down => SettingId::InputDevicesDPadDownMapping,
        BindingAction::Left => SettingId::InputDevicesDPadLeftMapping,
        BindingAction::Right => SettingId::InputDevicesDPadRightMapping,
        BindingAction::A => SettingId::InputDevicesAButtonMapping,
        BindingAction::B => SettingId::InputDevicesBButtonMapping,
        BindingAction::X => SettingId::InputDevicesXButtonMapping,
        BindingAction::Y => SettingId::InputDevicesYButtonMapping,
        BindingAction::L => SettingId::InputDevicesLShoulderMapping,
        BindingAction::R => SettingId::InputDevicesRShoulderMapping,
        BindingAction::Start => SettingId::InputDevicesStartButtonMapping,
        BindingAction::Select => SettingId::InputDevicesSelectButtonMapping,
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
    state.settings_ui.input_capture_scope = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_mapping_rows_fit_narrow_widths_with_inherit_actions() {
        for width in [340.0, 430.0, 464.0, 560.0] {
            for wonderswan in [false, true] {
                let context = egui::Context::default();
                let mut settings = Settings::default();
                crate::graphics::apply_egui_theme(
                    &context,
                    settings.ui.theme_preset,
                    settings.ui.ui_density,
                    settings.ui.debug_monospace_scale,
                    settings.ui.effective_debug_colors(),
                );
                let mut state = DebugWindowState::new();
                state.settings_ui.input_scope =
                    InputScope::System(crate::settings::InputSystem::GameBoyAdvance);
                let target = if wonderswan {
                    state.settings_ui.input_scope =
                        InputScope::System(crate::settings::InputSystem::WonderSwan);
                    BindingTarget::WonderSwan(crate::settings::WonderSwanButton::X2)
                } else {
                    BindingTarget::Joypad {
                        player: 1,
                        action: BindingAction::A,
                    }
                };
                settings
                    .set_binding(
                        &state.settings_ui.input_scope,
                        target,
                        GameplayBindingSource::Gamepad,
                        Some(PhysicalBinding::Gamepad("North".into())),
                    )
                    .unwrap();
                for _ in 0..2 {
                    let _ = context.run_ui(egui::RawInput::default(), |ui| {
                        ui.set_width(width);
                        let right = ui.max_rect().right();
                        if wonderswan {
                            super::super::wonderswan::draw_focused(
                                ui,
                                &mut settings,
                                &mut state,
                                BindingSource::Controller,
                            );
                        } else {
                            draw_focused(
                                ui,
                                &mut settings,
                                &mut state,
                                1,
                                BindingSource::Controller,
                                DiagramKind::StandardGamepad,
                            );
                        }
                        assert!(
                            ui.min_rect().right() <= right + 0.5,
                            "mapping overflow at {width}, WonderSwan={wonderswan}: {:?}",
                            ui.min_rect()
                        );
                    });
                }
            }
        }
    }

    #[test]
    fn keyboard_hotkey_collision_uses_the_shortcut_title() {
        let mut settings = Settings::default();
        let binding = PhysicalBinding::Keyboard(KeyCode::Space);
        assert_eq!(
            global_hotkey_conflicts(&settings, Some(&binding)),
            vec!["Speed-up key"]
        );
        settings.speedup_key = "KeyQ".into();
        assert!(global_hotkey_conflicts(&settings, Some(&binding)).is_empty());
    }

    #[test]
    fn gamepad_hotkey_collision_uses_the_action_title() {
        let mut settings = Settings::default();
        settings
            .gamepad_bindings
            .set_action(GamepadAction::Turbo, "South");
        let binding = PhysicalBinding::Gamepad("South".into());
        assert_eq!(
            global_hotkey_conflicts(&settings, Some(&binding)),
            vec!["Gamepad turbo action"]
        );
    }

    #[test]
    fn unbound_mapping_has_no_hotkey_collision() {
        let settings = Settings::default();
        assert!(global_hotkey_conflicts(&settings, None).is_empty());
        assert!(
            global_hotkey_conflicts(&settings, Some(&PhysicalBinding::Gamepad(String::new())))
                .is_empty()
        );
    }
}

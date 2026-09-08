use super::{BindingSource, controller_diagram::DiagramAction, joypad};
use crate::debug::DebugWindowState;
use crate::debug::settings_window::search::{self, SettingId};
use crate::settings::{BindingSet, BindingTarget, InputScope, Settings, WonderSwanButton};

struct WonderSwanBindingRow<'a> {
    settings: &'a mut Settings,
    state: &'a mut DebugWindowState,
    target: BindingTarget,
    gameplay_source: crate::settings::GameplayBindingSource,
    action: WonderSwanButton,
    diagram_action: DiagramAction,
    value: Option<BindingSet>,
    origin: InputScope,
}

pub(super) fn draw_focused(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    source: BindingSource,
) {
    if source == BindingSource::Controller {
        super::super::layout::helper(
            ui,
            "Additional direct mappings. The Player 1 D-pad already controls X horizontally and Y vertically. Unbound adds no extra mapping.",
        );
        ui.add_space(8.0);
    }
    joypad::draw_mapping_header(
        ui,
        source,
        state.settings_ui.input_scope != InputScope::Global,
    );

    for &action in WonderSwanButton::ALL {
        draw_binding_row(ui, settings, state, source, action);
    }

    ui.add_space(8.0);
    let restore = ui.button("Restore mappings for this scope…");
    search::target(ui, SettingId::InputDevicesRestoreDefaultMappings, &restore);
    if restore.clicked() {
        state.settings_ui.player_reset_confirmation = Some(match source {
            BindingSource::Keyboard => super::super::PlayerResetTarget::WonderSwanKeyboard,
            BindingSource::Controller => super::super::PlayerResetTarget::WonderSwanGamepad,
        });
    }
    joypad::draw_player_reset_confirmation(ui, settings, state);
    if source == BindingSource::Controller {
        let clear = ui.button("Unbind all direct mappings…");
        search::target(
            ui,
            SettingId::InputDevicesClearWonderswanDirectMappings,
            &clear,
        );
        if clear.clicked() {
            state.settings_ui.player_reset_confirmation =
                Some(super::super::PlayerResetTarget::WonderSwanClear);
        }
    }
}

fn draw_binding_row(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    source: BindingSource,
    action: WonderSwanButton,
) {
    let diagram_action = DiagramAction::WonderSwan(action);
    let target = BindingTarget::WonderSwan(action);
    let gameplay_source = joypad::gameplay_source(source);
    let resolved = settings.binding_set(&state.settings_ui.input_scope, target, gameplay_source);
    let highlighted = state.settings_ui.highlighted_mapping == Some(diagram_action);
    let mut row = WonderSwanBindingRow {
        settings,
        state,
        target,
        gameplay_source,
        action,
        diagram_action,
        value: resolved.value,
        origin: resolved.origin,
    };
    let available_width = ui.available_width();
    let layout = joypad::mapping_row_layout(
        ui,
        row.state.settings_ui.input_scope != InputScope::Global
            && row.origin == row.state.settings_ui.input_scope,
    );
    let shaded = available_width < 560.0;
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
        ui.spacing_mut().item_spacing.x = joypad::MAPPING_SPACING;
        if layout.stacked {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [joypad::MAPPING_LABEL_WIDTH, joypad::MAPPING_ROW_HEIGHT],
                    egui::Label::new(row.action.label()),
                );
                draw_binding_button(ui, layout.binding_width, &mut row);
                ui.add_sized(
                    [joypad::MAPPING_SOURCE_WIDTH, joypad::MAPPING_ROW_HEIGHT],
                    egui::Label::new(joypad::origin_label(&row.origin)).sense(egui::Sense::hover()),
                )
                .on_hover_text(format!(
                    "This value comes from the {} scope.",
                    joypad::origin_label(&row.origin)
                ));
            });
            ui.horizontal(|ui| {
                ui.add_space(joypad::MAPPING_LABEL_WIDTH);
                draw_binding_actions(ui, &mut row);
            });
        } else {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [joypad::MAPPING_LABEL_WIDTH, joypad::MAPPING_ROW_HEIGHT],
                    egui::Label::new(row.action.label()),
                );
                draw_binding_button(ui, layout.binding_width, &mut row);
                ui.add_sized(
                    [joypad::MAPPING_SOURCE_WIDTH, joypad::MAPPING_ROW_HEIGHT],
                    egui::Label::new(joypad::origin_label(&row.origin)).sense(egui::Sense::hover()),
                )
                .on_hover_text(format!(
                    "This value comes from the {} scope.",
                    joypad::origin_label(&row.origin)
                ));
                draw_binding_actions(ui, &mut row);
            });
        }
    });
}

fn draw_binding_button(ui: &mut egui::Ui, width: f32, row: &mut WonderSwanBindingRow<'_>) {
    let label = joypad::binding_set_label(row.value.as_ref());
    let binding = ui
        .add_sized(
            [width, joypad::MAPPING_ROW_HEIGHT],
            egui::Button::new(&label).truncate(),
        )
        .on_hover_text(format!("{label}\nOpen the binding editor"));
    search::target(ui, mapping_search_id(row.action), &binding);
    if binding.hovered() || binding.has_focus() {
        row.state.settings_ui.highlighted_mapping = Some(row.diagram_action);
    }
    if binding.clicked() {
        joypad::open_binding_editor(
            row.settings,
            row.state,
            row.target,
            row.gameplay_source,
            row.action.label().to_owned(),
        );
    }
}

fn draw_binding_actions(ui: &mut egui::Ui, row: &mut WonderSwanBindingRow<'_>) {
    let gameplay_conflict = binding_conflict(
        row.settings,
        &row.state.settings_ui.input_scope,
        row.action,
        row.gameplay_source,
    );
    let hotkey_conflicts = joypad::global_hotkey_conflicts_set(row.settings, row.value.as_ref());
    if gameplay_conflict.is_some() || !hotkey_conflicts.is_empty() {
        let mut message = gameplay_conflict
            .map(|other| {
                format!(
                    "Also assigned to {}. Existing first-match priority applies.",
                    other.label()
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
        joypad::edit_binding(
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
            joypad::edit_binding(
                row.settings,
                row.state,
                row.target,
                row.gameplay_source,
                true,
            );
        }
    }
}

fn binding_conflict(
    settings: &Settings,
    scope: &InputScope,
    current: WonderSwanButton,
    source: crate::settings::GameplayBindingSource,
) -> Option<WonderSwanButton> {
    let value = settings
        .binding_set(scope, BindingTarget::WonderSwan(current), source)
        .value?;
    WonderSwanButton::ALL.iter().copied().find(|&action| {
        action != current
            && settings
                .binding_set(scope, BindingTarget::WonderSwan(action), source)
                .value
                .is_some_and(|other| joypad::binding_sets_overlap(&value, &other))
    })
}

fn mapping_search_id(action: WonderSwanButton) -> SettingId {
    match action {
        WonderSwanButton::X1 => SettingId::InputDevicesWonderswanX1Mapping,
        WonderSwanButton::X2 => SettingId::InputDevicesWonderswanX2Mapping,
        WonderSwanButton::X3 => SettingId::InputDevicesWonderswanX3Mapping,
        WonderSwanButton::X4 => SettingId::InputDevicesWonderswanX4Mapping,
        WonderSwanButton::Y1 => SettingId::InputDevicesWonderswanY1Mapping,
        WonderSwanButton::Y2 => SettingId::InputDevicesWonderswanY2Mapping,
        WonderSwanButton::Y3 => SettingId::InputDevicesWonderswanY3Mapping,
        WonderSwanButton::Y4 => SettingId::InputDevicesWonderswanY4Mapping,
        WonderSwanButton::A => SettingId::InputDevicesWonderswanAMapping,
        WonderSwanButton::B => SettingId::InputDevicesWonderswanBMapping,
        WonderSwanButton::Start => SettingId::InputDevicesWonderswanStartMapping,
    }
}

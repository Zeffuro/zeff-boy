use super::MenuAction;
use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

pub(super) struct ToolbarState<'a> {
    pub(super) is_paused: bool,
    pub(super) active_system: ActiveSystem,
    pub(super) ws_display_rotated: bool,
    pub(super) speed_mode_label: Option<&'a str>,
    pub(super) active_save_slot: u8,
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    actions: &mut Vec<MenuAction>,
    settings: &mut Settings,
    state: ToolbarState<'_>,
) {
    let ToolbarState {
        is_paused,
        active_system,
        ws_display_rotated,
        speed_mode_label,
        active_save_slot,
    } = state;

    ui.label(
        egui::RichText::new(format!("Slot {active_save_slot}"))
            .small()
            .color(egui::Color32::LIGHT_GRAY),
    );
    ui.separator();

    let pause_icon = if is_paused { "▶" } else { "⏸" };
    let pause_tooltip = if is_paused {
        "Resume (F9)"
    } else {
        "Pause (F9)"
    };
    if ui
        .small_button(pause_icon)
        .on_hover_text(pause_tooltip)
        .clicked()
    {
        actions.push(MenuAction::TogglePause);
    }

    ui.separator();

    let mult = settings.emulation.fast_forward_multiplier;
    if ui
        .small_button("+")
        .on_hover_text("Increase speed multiplier")
        .clicked()
    {
        actions.push(MenuAction::SpeedChange(1));
    }
    ui.label(
        egui::RichText::new(format!("{mult}×"))
            .small()
            .color(egui::Color32::LIGHT_GRAY),
    );
    if ui
        .small_button("−")
        .on_hover_text("Decrease speed multiplier")
        .clicked()
    {
        actions.push(MenuAction::SpeedChange(-1));
    }

    ui.separator();

    if let Some(label) = speed_mode_label {
        ui.label(
            egui::RichText::new(label)
                .small()
                .color(egui::Color32::LIGHT_GRAY),
        );
        ui.separator();
    }

    if active_system == ActiveSystem::WonderSwan {
        let label = if ws_display_rotated { "Rot V" } else { "Rot H" };
        if ui
            .small_button(label)
            .on_hover_text("Rotate WonderSwan display")
            .clicked()
        {
            actions.push(MenuAction::ToggleWsRotation);
        }
        ui.separator();
    }

    let muted = settings.audio.volume <= 0.001;
    let icon = if muted { "🔇" } else { "🔊" };
    if ui.small_button(icon).clicked() {
        if muted {
            settings.audio.volume = settings.audio.pre_mute_volume.take().unwrap_or(1.0);
        } else {
            settings.audio.pre_mute_volume = Some(settings.audio.volume);
            settings.audio.volume = 0.0;
        }
        actions.push(MenuAction::ToolbarSettingsChanged);
    }

    let vol_before = settings.audio.volume;
    ui.spacing_mut().slider_width = 80.0;
    ui.add(
        egui::Slider::new(&mut settings.audio.volume, 0.0..=1.0)
            .show_value(false)
            .text(""),
    );
    if (settings.audio.volume - vol_before).abs() > f32::EPSILON {
        actions.push(MenuAction::ToolbarSettingsChanged);
    }
}

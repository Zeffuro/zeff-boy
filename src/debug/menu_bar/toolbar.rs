use super::MenuAction;
use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

const VOLUME_SLIDER_WIDTH: f32 = 80.0;
const SEPARATOR_WIDTH: f32 = 6.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ToolbarLayout {
    slot: bool,
    pause: bool,
    speed: bool,
    speed_mode: bool,
    rotation: bool,
    mute: bool,
    volume: bool,
}

#[derive(Clone, Copy, Default)]
struct ToolbarWidths {
    slot: f32,
    pause: f32,
    speed: f32,
    speed_mode: f32,
    rotation: f32,
    mute: f32,
    volume: f32,
    item_spacing: f32,
}

impl ToolbarLayout {
    fn select(available_width: f32, widths: ToolbarWidths, state: &ToolbarState<'_>) -> Self {
        let mut layout = Self {
            slot: true,
            pause: true,
            speed: true,
            speed_mode: state.speed_mode_label.is_some(),
            rotation: state.active_system == ActiveSystem::WonderSwan,
            mute: true,
            volume: true,
        };

        for hide in [
            |layout: &mut Self| layout.speed_mode = false,
            |layout: &mut Self| layout.slot = false,
            |layout: &mut Self| layout.rotation = false,
            |layout: &mut Self| layout.volume = false,
            |layout: &mut Self| layout.speed = false,
            |layout: &mut Self| layout.mute = false,
            |layout: &mut Self| layout.pause = false,
        ] {
            if layout.width(widths) <= available_width {
                break;
            }
            hide(&mut layout);
        }

        layout
    }

    fn width(self, widths: ToolbarWidths) -> f32 {
        let groups = [
            (self.slot, widths.slot, 1usize),
            (self.pause, widths.pause, 1),
            (self.speed, widths.speed, 3),
            (self.speed_mode, widths.speed_mode, 1),
            (self.rotation, widths.rotation, 1),
            (self.mute, widths.mute, 1),
            (self.volume, widths.volume, 1),
        ];
        let visible_groups = groups.iter().filter(|(visible, _, _)| *visible).count();
        let control_width: f32 = groups
            .iter()
            .filter(|(visible, _, _)| *visible)
            .map(|(_, width, _)| width)
            .sum();
        let control_count: usize = groups
            .iter()
            .filter(|(visible, _, _)| *visible)
            .map(|(_, _, count)| count)
            .sum();
        let separators = visible_groups.saturating_sub(1);
        let widget_count = control_count + separators;

        control_width
            + separators as f32 * SEPARATOR_WIDTH
            + widget_count.saturating_sub(1) as f32 * widths.item_spacing
    }

    fn after_slot(self) -> bool {
        self.pause || self.speed || self.speed_mode || self.rotation || self.mute || self.volume
    }

    fn after_pause(self) -> bool {
        self.speed || self.speed_mode || self.rotation || self.mute || self.volume
    }

    fn after_speed(self) -> bool {
        self.speed_mode || self.rotation || self.mute || self.volume
    }

    fn after_speed_mode(self) -> bool {
        self.rotation || self.mute || self.volume
    }

    fn after_rotation(self) -> bool {
        self.mute || self.volume
    }
}

pub(super) struct ToolbarState<'a> {
    pub(super) is_paused: bool,
    pub(super) active_system: ActiveSystem,
    pub(super) ws_display_rotated: bool,
    pub(super) speed_mode_label: Option<&'a str>,
    pub(super) active_save_slot: u8,
    pub(super) reserved_width: f32,
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    actions: &mut Vec<MenuAction>,
    settings: &mut Settings,
    state: ToolbarState<'_>,
) {
    let widths = measure(ui, settings, &state);
    let layout = ToolbarLayout::select(
        (ui.available_width() - state.reserved_width).max(0.0),
        widths,
        &state,
    );
    let ToolbarState {
        is_paused,
        active_system,
        ws_display_rotated,
        speed_mode_label,
        active_save_slot,
        reserved_width: _,
    } = state;

    if layout.slot {
        ui.label(
            egui::RichText::new(format!("Slot {active_save_slot}"))
                .small()
                .color(egui::Color32::LIGHT_GRAY),
        );
        if layout.after_slot() {
            ui.separator();
        }
    }

    if layout.pause {
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
        if layout.after_pause() {
            ui.separator();
        }
    }

    if layout.speed {
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
        if layout.after_speed() {
            ui.separator();
        }
    }

    if layout.speed_mode {
        if let Some(label) = speed_mode_label {
            ui.label(
                egui::RichText::new(label)
                    .small()
                    .color(egui::Color32::LIGHT_GRAY),
            );
        }
        if layout.after_speed_mode() {
            ui.separator();
        }
    }

    if layout.rotation {
        debug_assert_eq!(active_system, ActiveSystem::WonderSwan);
        let label = if ws_display_rotated { "Rot V" } else { "Rot H" };
        if ui
            .small_button(label)
            .on_hover_text("Rotate WonderSwan display")
            .clicked()
        {
            actions.push(MenuAction::ToggleWsRotation);
        }
        if layout.after_rotation() {
            ui.separator();
        }
    }

    if layout.mute {
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
        if layout.volume {
            ui.separator();
        }
    }

    if layout.volume {
        let vol_before = settings.audio.volume;
        ui.spacing_mut().slider_width = VOLUME_SLIDER_WIDTH;
        ui.add(
            egui::Slider::new(&mut settings.audio.volume, 0.0..=1.0)
                .show_value(false)
                .text(""),
        );
        if (settings.audio.volume - vol_before).abs() > f32::EPSILON {
            actions.push(MenuAction::ToolbarSettingsChanged);
        }
    }
}

pub(super) fn essential_width(ui: &egui::Ui, settings: &Settings, state: &ToolbarState<'_>) -> f32 {
    measure(ui, settings, state).pause
}

fn measure(ui: &egui::Ui, settings: &Settings, state: &ToolbarState<'_>) -> ToolbarWidths {
    let small = egui::TextStyle::Small.resolve(ui.style());
    let button = egui::TextStyle::Button.resolve(ui.style());
    let text_width = |text: &str, font: egui::FontId| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
            .size()
            .x
    };
    let button_width =
        |text: &str| text_width(text, button.clone()) + 2.0 * ui.spacing().button_padding.x;
    let pause = button_width(if state.is_paused { "▶" } else { "⏸" });
    let mute = button_width(if settings.audio.volume <= 0.001 {
        "🔇"
    } else {
        "🔊"
    });
    let mult = format!("{}×", settings.emulation.fast_forward_multiplier);

    ToolbarWidths {
        slot: text_width(&format!("Slot {}", state.active_save_slot), small.clone()),
        pause,
        speed: button_width("+") + text_width(&mult, small) + button_width("−"),
        speed_mode: state.speed_mode_label.map_or(0.0, |label| {
            text_width(label, egui::TextStyle::Small.resolve(ui.style()))
        }),
        rotation: button_width(if state.ws_display_rotated {
            "Rot V"
        } else {
            "Rot H"
        }),
        mute,
        volume: VOLUME_SLIDER_WIDTH,
        item_spacing: ui.spacing().item_spacing.x,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ToolbarState<'static> {
        ToolbarState {
            is_paused: false,
            active_system: ActiveSystem::GameBoy,
            ws_display_rotated: false,
            speed_mode_label: Some("Normal"),
            active_save_slot: 1,
            reserved_width: 0.0,
        }
    }

    fn widths() -> ToolbarWidths {
        ToolbarWidths {
            slot: 36.0,
            pause: 18.0,
            speed: 50.0,
            speed_mode: 40.0,
            rotation: 38.0,
            mute: 18.0,
            volume: 80.0,
            item_spacing: 4.0,
        }
    }

    #[test]
    fn toolbar_collapses_one_group_at_a_time() {
        let state = state();
        let widths = widths();
        let full = ToolbarLayout::select(f32::INFINITY, widths, &state);
        let without_mode = ToolbarLayout::select(full.width(widths) - 1.0, widths, &state);
        let without_slot = ToolbarLayout::select(without_mode.width(widths) - 1.0, widths, &state);

        assert!(full.speed_mode && full.slot && full.volume);
        assert!(!without_mode.speed_mode && without_mode.slot && without_mode.volume);
        assert!(!without_slot.speed_mode && !without_slot.slot && without_slot.volume);
    }

    #[test]
    fn toolbar_keeps_pause_until_last() {
        let layout = ToolbarLayout::select(widths().pause, widths(), &state());
        assert!(layout.pause);
        assert!(!layout.slot && !layout.speed && !layout.mute && !layout.volume);
    }
}

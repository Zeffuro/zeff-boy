use super::*;
use crate::ui::TraceTarget;

pub(super) fn draw(ui: &mut egui::Ui, state: &mut TraceViewerState, actions: &mut DebugUiActions) {
    let mut target = state.target;
    if state.sound.is_some() {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut target, TraceTarget::Primary, "PRIMARY M68000");
            ui.selectable_value(&mut target, TraceTarget::Sound, "SOUND Z80");
        });
        state.select_target(target);
    }
    ui.horizontal_wrapped(|ui| {
        let label = if state.enabled { "Stop" } else { "Record" };
        let admitted = state.target == TraceTarget::Primary
            || state.enabled
            || state.sound.is_some_and(|sound| sound.rejection.is_none());
        if ui.add_enabled(admitted, egui::Button::new(label)).clicked() {
            if state.target == TraceTarget::Sound {
                actions.sound_trace.enabled = Some(!state.enabled);
            } else {
                actions.trace_enabled = Some(!state.enabled);
            }
        }
        if ui.button("Clear").clicked() {
            if state.target == TraceTarget::Sound {
                actions.sound_trace.clear = true;
            } else {
                actions.trace_clear = true;
            }
            state.clear();
        }
        egui::ComboBox::from_id_salt("trace_capacity")
            .selected_text(format_capacity(state.capacity))
            .show_ui(ui, |ui| {
                for capacity in [1_000, 10_000, 50_000, 100_000] {
                    if ui
                        .selectable_label(state.capacity == capacity, format_capacity(capacity))
                        .clicked()
                    {
                        if state.target == TraceTarget::Sound {
                            actions.sound_trace.capacity = Some(capacity);
                        } else {
                            actions.trace_capacity = Some(capacity);
                        }
                    }
                }
            });
        ui.checkbox(&mut state.auto_scroll, "Follow");
        ui.label(format!("{} / {} retained", state.retained, state.capacity));
        if state.missed != 0 {
            ui.colored_label(
                crate::debug::common::color32(crate::debug::common::debug_colors(ui).breakpoint),
                format!("{} missed", state.missed),
            );
        }
    });

    if state.target == TraceTarget::Sound
        && let Some(sound) = state.sound
    {
        if let Some(reason) = sound.rejection {
            ui.label(reason);
        }
        if let Some(gap) = sound.gap {
            ui.weak(gap);
        }
        ui.weak("Passive observation; SOUND execution controls and MAIN symbols are unavailable");
    }
}

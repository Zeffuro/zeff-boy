use crate::debug::DebugWindowState;
use crate::settings::{Settings, ShortcutAction};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings, state: &mut DebugWindowState) {
    egui::CollapsingHeader::new("Shortcuts")
        .default_open(true)
        .show(ui, |ui| {
            if state.rebinding_shortcut.is_some()
                || state.rebinding_speedup
                || state.rebinding_rewind
            {
                ui.label(
                    egui::RichText::new("Press a key to rebind...").color(egui::Color32::YELLOW),
                );
            }
            egui::Grid::new("shortcuts_grid")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Speed-up (hold)");
                    let key_label = if state.rebinding_speedup {
                        format!("Press key... ({})", settings.speedup_key)
                    } else {
                        settings.speedup_key.clone()
                    };
                    if ui.button(key_label).clicked() {
                        state.rebinding_speedup = true;
                        state.rebinding_action = None;
                        state.rebinding_shortcut = None;
                        state.rebinding_rewind = false;
                    }
                    ui.end_row();

                    ui.label("Rewind (hold)");
                    let rewind_label = if state.rebinding_rewind {
                        format!("Press key... ({})", settings.rewind.key)
                    } else {
                        settings.rewind.key.clone()
                    };
                    if ui.button(rewind_label).clicked() {
                        state.rebinding_rewind = true;
                        state.rebinding_action = None;
                        state.rebinding_shortcut = None;
                        state.rebinding_speedup = false;
                    }
                    ui.end_row();

                    for &action in ShortcutAction::ALL {
                        ui.label(action.label());
                        let key_str = settings.shortcut_bindings.key_str(action).to_owned();
                        let capture_label = if state.rebinding_shortcut == Some(action) {
                            format!("Press key... ({key_str})")
                        } else {
                            key_str
                        };
                        if ui.button(capture_label).clicked() {
                            state.rebinding_shortcut = Some(action);
                            state.rebinding_action = None;
                            state.rebinding_speedup = false;
                            state.rebinding_rewind = false;
                        }
                        ui.end_row();
                    }
                });
            ui.label(
                egui::RichText::new(
                    "Additional hardcoded bindings: Ctrl-R = Reset, \
                     Alt-Enter = Fullscreen, ` = Speed-up, \
                     LShift = Turbo (rapid-fire), 0-9 = select save slot",
                )
                .weak()
                .small(),
            );
        });
}

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
                        "Press a key…".to_owned()
                    } else {
                        super::joypad::key_label(settings.speedup_key_code())
                    };
                    if ui
                        .add_sized([170.0, 26.0], egui::Button::new(key_label))
                        .clicked()
                    {
                        super::joypad::clear_capture(state);
                        state.rebinding_speedup = true;
                    }
                    ui.end_row();

                    ui.label("Rewind (hold)");
                    let rewind_label = if state.rebinding_rewind {
                        "Press a key…".to_owned()
                    } else {
                        super::joypad::key_label(settings.rewind.key_code())
                    };
                    if ui
                        .add_sized([170.0, 26.0], egui::Button::new(rewind_label))
                        .clicked()
                    {
                        super::joypad::clear_capture(state);
                        state.rebinding_rewind = true;
                    }
                    ui.end_row();

                    for &action in ShortcutAction::ALL {
                        ui.label(action.label());
                        let capture_label = if state.rebinding_shortcut == Some(action) {
                            "Press a key…".to_owned()
                        } else {
                            super::joypad::key_label(settings.shortcut_bindings.get(action))
                        };
                        if ui
                            .add_sized([170.0, 26.0], egui::Button::new(capture_label))
                            .clicked()
                        {
                            super::joypad::clear_capture(state);
                            state.rebinding_shortcut = Some(action);
                        }
                        ui.end_row();
                    }
                });
        });
}

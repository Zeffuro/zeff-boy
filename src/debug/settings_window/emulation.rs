use crate::hardware::types::hardware_mode::HardwareModePreference;
use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Hardware");
    egui::ComboBox::from_label("Hardware mode")
        .selected_text(match settings.hardware_mode_preference {
            HardwareModePreference::Auto => "Auto",
            HardwareModePreference::ForceDmg => "DMG",
            HardwareModePreference::ForceCgb => "CGB",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut settings.hardware_mode_preference,
                HardwareModePreference::Auto,
                "Auto",
            );
            ui.selectable_value(
                &mut settings.hardware_mode_preference,
                HardwareModePreference::ForceDmg,
                "DMG",
            );
            ui.selectable_value(
                &mut settings.hardware_mode_preference,
                HardwareModePreference::ForceCgb,
                "CGB",
            );
        });

    ui.separator();
    ui.heading("Speed");
    ui.add(
        egui::Slider::new(&mut settings.fast_forward_multiplier, 1..=16)
            .text("Fast-forward multiplier"),
    );
    ui.add(
        egui::Slider::new(&mut settings.uncapped_frames_per_tick, 1..=240)
            .text("Uncapped frames/tick"),
    );
    ui.checkbox(&mut settings.uncapped_speed, "Start in uncapped mode");
    ui.checkbox(&mut settings.frame_skip, "Frame skip when behind")
        .on_hover_text(
            "When enabled, skip emulation frames to stay in real-time if the \
             host can't keep up. When disabled, the emulator catches up \
             gradually (more accurate, may drift behind).",
        );
    ui.checkbox(&mut settings.auto_save_state, "Auto save/load state")
        .on_hover_text(
            "Automatically save emulator state when closing and \
             restore it when loading the same ROM.",
        );

    ui.separator();
    ui.heading("Rewind");
    ui.checkbox(&mut settings.rewind_enabled, "Enable rewind")
        .on_hover_text(
            "Hold the rewind key to rewind gameplay. \
             Captures a snapshot every 4 frames (~15 fps capture rate).",
        );
    ui.horizontal(|ui| {
        ui.label("History (seconds):");
        ui.add(
            egui::DragValue::new(&mut settings.rewind_seconds)
                .range(1..=120)
                .speed(1),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Rewind speed:");
        ui.add(
            egui::DragValue::new(&mut settings.rewind_speed)
                .range(1..=10)
                .speed(1),
        );
        ui.label(match settings.rewind_speed {
            1 => "(fastest — pop every tick)",
            2 => "(fast)",
            3..=4 => "(normal)",
            _ => "(slow)",
        });
    });
}


use crate::debug::common::InputDebugInfo;

pub(super) fn draw_input_viewer_content(ui: &mut egui::Ui, info: &InputDebugInfo) {
    for section in &info.sections {
        ui.heading(&section.heading);
        for line in &section.lines {
            ui.monospace(line);
        }
        ui.separator();
    }

    for (label, value) in &info.progress_bars {
        ui.add(
            egui::ProgressBar::new(*value)
                .show_percentage()
                .text(label.as_str()),
        );
    }
}

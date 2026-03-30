use crate::debug::types::OamDebugInfo;

pub(super) fn draw_oam_viewer_content(ui: &mut egui::Ui, info: &OamDebugInfo) {
    egui::Grid::new("oam_grid").striped(true).show(ui, |ui| {
        for header in &info.headers {
            ui.strong(header);
        }
        ui.end_row();

        for row in &info.rows {
            for cell in row {
                ui.monospace(cell);
            }
            ui.end_row();
        }
    });
}

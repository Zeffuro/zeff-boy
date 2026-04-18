use crate::debug::types::PaletteDebugInfo;

pub(super) fn draw_palette_viewer_content(ui: &mut egui::Ui, info: &PaletteDebugInfo) {
    for group in &info.groups {
        ui.label(group.title.as_ref());
        for row in &group.rows {
            ui.horizontal(|ui| {
                ui.label(&row.label);
                for rgba in &row.colors {
                    let color =
                        egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                    let luminance = (rgba[0] as u16 + rgba[1] as u16 + rgba[2] as u16) / 3;
                    let text_color = if luminance < 128 {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::BLACK
                    };
                    egui::Frame::NONE.fill(color).show(ui, |ui| {
                        ui.add_sized(
                            [28.0, 20.0],
                            egui::Label::new(egui::RichText::new("").color(text_color)),
                        );
                    });
                }
            });
        }
        ui.separator();
    }
}

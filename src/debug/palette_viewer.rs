use crate::debug::common::PaletteDebugInfo;
use zeff_gb_core::hardware::ppu::{PALETTE_COLORS, apply_palette, cgb_palette_rgba, correct_color};
use crate::settings::ColorCorrection;

fn draw_palette_row(ui: &mut egui::Ui, label: &str, value: u8) {
    ui.label(format!("{} ({:02X})", label, value));
    ui.horizontal(|ui| {
        for color_id in 0..4u8 {
            let rgba = apply_palette(value, color_id);
            let color = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            let text_color = if color_id >= 2 {
                egui::Color32::WHITE
            } else {
                egui::Color32::BLACK
            };
            egui::Frame::NONE.fill(color).show(ui, |ui| {
                ui.add_sized(
                    [36.0, 24.0],
                    egui::Label::new(
                        egui::RichText::new(format!("{}", color_id)).color(text_color),
                    ),
                );
            });
        }
    });
}

fn draw_cgb_palette_section(
    ui: &mut egui::Ui,
    title: &str,
    row_prefix: &str,
    palette_ram: &[u8; 64],
    color_correction: ColorCorrection,
    color_correction_matrix: [f32; 9],
) {
    ui.separator();
    ui.label(title);
    for palette in 0u8..8 {
        ui.horizontal(|ui| {
            ui.label(format!("{}{}", row_prefix, palette));
            for color_id in 0u8..4 {
                let rgba = correct_color(
                    cgb_palette_rgba(palette_ram, palette, color_id),
                    color_correction,
                    color_correction_matrix,
                );
                let color =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                egui::Frame::NONE.fill(color).show(ui, |ui| {
                    ui.add_sized([24.0, 16.0], egui::Label::new(""));
                });
            }
        });
    }
}

pub(super) fn draw_palette_viewer_content(
    ui: &mut egui::Ui,
    info: &PaletteDebugInfo,
) {
    for group in &info.groups {
        ui.label(&group.title);
        for row in &group.rows {
            ui.horizontal(|ui| {
                ui.label(&row.label);
                for rgba in &row.colors {
                    let color = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
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

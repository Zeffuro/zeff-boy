use crate::hardware::ppu::{PALETTE_COLORS, apply_palette, cgb_palette_rgba};

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
) {
    ui.separator();
    ui.label(title);
    for palette in 0u8..8 {
        ui.horizontal(|ui| {
            ui.label(format!("{}{}", row_prefix, palette));
            for color_id in 0u8..4 {
                let rgba = cgb_palette_rgba(palette_ram, palette, color_id);
                let color =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                egui::Frame::NONE.fill(color).show(ui, |ui| {
                    ui.add_sized([24.0, 16.0], egui::Label::new(""));
                });
            }
        });
    }
}

pub(crate) fn draw_palette_viewer(
    ctx: &egui::Context,
    bgp: u8,
    obp0: u8,
    obp1: u8,
    cgb_mode: bool,
    bg_palette_ram: &[u8; 64],
    obj_palette_ram: &[u8; 64],
    open: &mut bool,
) {
    egui::Window::new("Palettes")
        .open(open)
        .default_width(280.0)
        .show(ctx, |ui| {
            draw_palette_row(ui, "BGP", bgp);
            ui.separator();
            draw_palette_row(ui, "OBP0", obp0);
            ui.separator();
            draw_palette_row(ui, "OBP1", obp1);
            ui.separator();
            ui.label("Base DMG shades:");
            ui.horizontal(|ui| {
                for rgba in PALETTE_COLORS {
                    let color =
                        egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                    egui::Frame::NONE.fill(color).show(ui, |ui| {
                        ui.add_space(24.0);
                        ui.add_sized([24.0, 16.0], egui::Label::new(""));
                    });
                }
            });

            if cgb_mode {
                draw_cgb_palette_section(ui, "CGB BG palettes:", "BG", bg_palette_ram);
                draw_cgb_palette_section(ui, "CGB OBJ palettes:", "OB", obj_palette_ram);
            }
        });
}

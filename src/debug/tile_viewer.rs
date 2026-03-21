use crate::hardware::ppu::apply_palette;

pub(crate) fn draw_tile_viewer(ctx: &egui::Context, vram: &[u8], bgp: u8, open: &mut bool) {
    egui::Window::new("Tile Data")
        .open(open)
        .default_width(320.0)
        .show(ctx, |ui| {
            let width = 16 * 8;
            let height = 24 * 8;
            let mut image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);

            for tile in 0..384usize {
                let tile_x = tile % 16;
                let tile_y = tile / 16;
                let tile_addr = tile * 16;

                for y in 0..8usize {
                    let lo = vram.get(tile_addr + y * 2).copied().unwrap_or(0);
                    let hi = vram.get(tile_addr + y * 2 + 1).copied().unwrap_or(0);

                    for x in 0..8usize {
                        let bit = 7 - x as u8;
                        let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                        let rgba = apply_palette(bgp, color_id);
                        let px = tile_x * 8 + x;
                        let py = tile_y * 8 + y;
                        image[(px, py)] = egui::Color32::from_rgba_unmultiplied(
                            rgba[0], rgba[1], rgba[2], rgba[3],
                        );
                    }
                }
            }

            let texture = ui.ctx().load_texture("tile_viewer", image, egui::TextureOptions::NEAREST);
            let display_size = egui::vec2((width as f32) * 2.0, (height as f32) * 2.0);
            egui::ScrollArea::both().show(ui, |ui| {
                ui.image((texture.id(), display_size));
            });
        });
}


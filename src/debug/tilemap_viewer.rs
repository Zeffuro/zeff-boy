use crate::debug::PpuSnapshot;
use crate::hardware::ppu::apply_palette;

pub(crate) fn draw_tilemap_viewer(ctx: &egui::Context, vram: &[u8], ppu: PpuSnapshot, open: &mut bool) {
    egui::Window::new("Tile Map")
        .open(open)
        .default_width(320.0)
        .show(ctx, |ui| {
            let width = 256usize;
            let height = 256usize;
            let tile_map_base = if ppu.lcdc & 0x08 != 0 { 0x1C00 } else { 0x1800 };
            let tile_data_unsigned = ppu.lcdc & 0x10 != 0;
            let mut image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);

            for y in 0..height {
                let tile_row = y / 8;
                let line_in_tile = y % 8;
                for x in 0..width {
                    let tile_col = x / 8;
                    let tile_map_addr = tile_map_base + tile_row * 32 + tile_col;
                    let tile_index = vram.get(tile_map_addr).copied().unwrap_or(0);
                    let tile_data_addr = if tile_data_unsigned {
                        (tile_index as usize) * 16
                    } else {
                        ((tile_index as i8 as i16 + 128) as usize) * 16
                    };

                    let lo = vram.get(tile_data_addr + line_in_tile * 2).copied().unwrap_or(0);
                    let hi = vram.get(tile_data_addr + line_in_tile * 2 + 1).copied().unwrap_or(0);
                    let bit = 7 - (x % 8) as u8;
                    let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                    let rgba = apply_palette(ppu.bgp, color_id);
                    image[(x, y)] =
                        egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                }
            }

            let texture = ui
                .ctx()
                .load_texture("tilemap_viewer", image, egui::TextureOptions::NEAREST);
            egui::ScrollArea::both().show(ui, |ui| {
                ui.image((texture.id(), egui::vec2((width as f32) * 1.5, (height as f32) * 1.5)));
            });
        });
}


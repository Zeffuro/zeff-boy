use crate::debug::TilemapViewerState;
use crate::debug::common::nes_palette_rgba;
use crate::debug::types::NesGraphicsData;

use super::tilemap_viewer::{ViewportOverlay, draw_wrapped_viewport_rect};

fn nes_attr_palette(nametable_data: &[u8], nt_offset: usize, tile_col: usize, tile_row: usize) -> (u8, u8) {
    let attr_col = tile_col / 4;
    let attr_row = tile_row / 4;
    let attr_addr = nt_offset + 0x3C0 + attr_row * 8 + attr_col;
    let attr_byte = nametable_data.get(attr_addr).copied().unwrap_or(0);
    let shift = ((tile_col / 2) & 1) * 2 + ((tile_row / 2) & 1) * 4;
    (attr_byte, (attr_byte >> shift) & 0x03)
}

pub(super) fn draw_nes_tilemap_viewer_content(
    ui: &mut egui::Ui,
    gfx: &NesGraphicsData,
    window_state: &mut TilemapViewerState,
) {
    let mirroring_label = format!("Mirroring: {:?}", gfx.mirroring);
    ui.label(&mirroring_label);

    let show_viewport_id = ui.make_persistent_id("nes_tilemap_show_viewport");
    let show_viewport = super::common::persisted_checkbox(ui, show_viewport_id, "Show screen viewport", true);

    let width = 512usize;
    let height = 480usize;

    if window_state.image.size != [width, height] {
        window_state.image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        window_state.tracker.vram_dirty = true;
    }

    window_state.tracker.vram_dirty = true;

    if window_state.tracker.vram_dirty {
        render_nes_nametables(&mut window_state.image, gfx);
        window_state.tracker.vram_dirty = false;
    }

    let texture = window_state.texture.get_or_insert_with(|| {
        ui.ctx().load_texture(
            "nes_tilemap_viewer",
            window_state.image.clone(),
            egui::TextureOptions::NEAREST,
        )
    });
    texture.set(window_state.image.clone(), egui::TextureOptions::NEAREST);

    let display_size = egui::vec2(width as f32, height as f32);
    ui.horizontal(|ui| {
        super::export::export_png_button(ui, "nes_nametable.png", &window_state.image);
    });

    egui::ScrollArea::both().show(ui, |ui| {
        let response = ui.image((texture.id(), display_size));

        if show_viewport {
            let scale_x = response.rect.width() / width as f32;
            let scale_y = response.rect.height() / height as f32;
            let origin = response.rect.min;

            let v = gfx.scroll_t;
            let coarse_x = (v & 0x001F) as f32;
            let coarse_y = ((v >> 5) & 0x001F) as f32;
            let fine_y = ((v >> 12) & 0x07) as f32;
            let nt_select = ((v >> 10) & 0x03) as f32;
            let nt_x = nt_select as u16 & 1;
            let nt_y = (nt_select as u16 >> 1) & 1;

            let scroll_x = (nt_x as f32) * 256.0 + coarse_x * 8.0 + gfx.fine_x as f32;
            let scroll_y = (nt_y as f32) * 240.0 + coarse_y * 8.0 + fine_y;

            let painter = ui.painter_at(response.rect);
            draw_wrapped_viewport_rect(
                &painter,
                &ViewportOverlay {
                    origin,
                    scale_x,
                    scale_y,
                    scroll_x,
                    scroll_y,
                    viewport_w: 256.0,
                    viewport_h: 240.0,
                    map_w: width as f32,
                    map_h: height as f32,
                    color: egui::Color32::from_rgba_unmultiplied(0, 255, 0, 200),
                },
            );
        }

        if let Some((px, py)) = super::common::hover_pixel_coords(&response, width, height) {
            let nt_quad = (px / 256) + (py / 240) * 2;
            let local_x = px % 256;
            let local_y = py % 240;
            let tile_col = local_x / 8;
            let tile_row = local_y / 8;
            let nt_offset = nt_quad * 0x400;
            let tile_idx_addr = nt_offset + tile_row * 32 + tile_col;
            let tile_index = gfx.nametable_data.get(tile_idx_addr).copied().unwrap_or(0);
            let (attr_byte, palette) = nes_attr_palette(&gfx.nametable_data, nt_offset, tile_col, tile_row);

            ui.separator();
            ui.monospace(format!(
                "NT{} ({:3},{:3}) tile:{:02X} attr:{:02X} pal:{}",
                nt_quad, local_x, local_y, tile_index, attr_byte, palette,
            ));
        }
    });
}

fn render_nes_nametables(image: &mut egui::ColorImage, gfx: &NesGraphicsData) {
    let bg_pattern_base: usize = if gfx.ctrl & 0x10 != 0 { 0x1000 } else { 0x0000 };

    for nt_quad in 0..4usize {
        let nt_offset = nt_quad * 0x400;
        let quad_x = (nt_quad % 2) * 256;
        let quad_y = (nt_quad / 2) * 240;

        for tile_row in 0..30usize {
            for tile_col in 0..32usize {
                let tile_index = gfx
                    .nametable_data
                    .get(nt_offset + tile_row * 32 + tile_col)
                    .copied()
                    .unwrap_or(0) as usize;

                let (_, palette_index) = nes_attr_palette(&gfx.nametable_data, nt_offset, tile_col, tile_row);

                let tile_addr = bg_pattern_base + tile_index * 16;
                for row in 0..8usize {
                    for col in 0..8usize {
                        let color_id = super::nes_tile_viewer::decode_nes_tile_pixel(&gfx.chr_data, tile_addr, row, col);
                        let rgba = nes_palette_rgba(
                            &gfx.palette_ram,
                            palette_index,
                            color_id,
                            gfx.palette_mode,
                        );
                        let px = quad_x + tile_col * 8 + col;
                        let py = quad_y + tile_row * 8 + row;
                        if px < image.size[0] && py < image.size[1] {
                            image[(px, py)] = egui::Color32::from_rgba_unmultiplied(
                                rgba[0], rgba[1], rgba[2], rgba[3],
                            );
                        }
                    }
                }
            }
        }
    }
}

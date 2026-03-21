use crate::debug::PpuSnapshot;
use crate::hardware::ppu::{apply_palette, cgb_palette_rgba, decode_tile_pixel, tile_data_address};

#[derive(Clone, Copy)]
struct TileAttrInfo {
    palette: u8,
    vram_bank: u8,
    flip_x: bool,
    flip_y: bool,
    priority: bool,
}

fn decode_tile_attr(attr: u8) -> TileAttrInfo {
    TileAttrInfo {
        palette: attr & 0x07,
        vram_bank: (attr >> 3) & 0x01,
        flip_x: attr & 0x20 != 0,
        flip_y: attr & 0x40 != 0,
        priority: attr & 0x80 != 0,
    }
}

pub(crate) fn draw_tilemap_viewer(
    ctx: &egui::Context,
    vram: &[u8],
    ppu: PpuSnapshot,
    cgb_mode: bool,
    bg_palette_ram: &[u8; 64],
    open: &mut bool,
) {
    egui::Window::new("Tile Map")
        .open(open)
        .default_width(320.0)
        .show(ctx, |ui| {
            let width = 256usize;
            let height = 256usize;
            let bg_tile_map_base = if ppu.lcdc & 0x08 != 0 { 0x1C00 } else { 0x1800 };
            let win_tile_map_base = if ppu.lcdc & 0x40 != 0 { 0x1C00 } else { 0x1800 };

            let map_select_id = ui.make_persistent_id("tilemap_viewer_source");
            let mut use_window_map = ui
                .ctx()
                .data_mut(|d| d.get_persisted::<bool>(map_select_id))
                .unwrap_or(false);

            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut use_window_map,
                    false,
                    format!("BG map (LCDC.3): 0x{:04X}", 0x8000 + bg_tile_map_base),
                );
                ui.selectable_value(
                    &mut use_window_map,
                    true,
                    format!("Window map (LCDC.6): 0x{:04X}", 0x8000 + win_tile_map_base),
                );
            });
            ui.ctx()
                .data_mut(|d| d.insert_persisted(map_select_id, use_window_map));

            let attr_overlay_id = ui.make_persistent_id("tilemap_viewer_attr_overlay");
            let mut show_attr_overlay = ui
                .ctx()
                .data_mut(|d| d.get_persisted::<bool>(attr_overlay_id))
                .unwrap_or(false);
            let cgb_attr_available = cgb_mode && vram.len() >= 0x4000;

            let render_cgb_color_id = ui.make_persistent_id("tilemap_viewer_cgb_colors");
            let mut render_cgb_colors = ui
                .ctx()
                .data_mut(|d| d.get_persisted::<bool>(render_cgb_color_id))
                .unwrap_or(true);
            if !cgb_attr_available {
                render_cgb_colors = false;
            }

            ui.horizontal(|ui| {
                ui.add_enabled(
                    cgb_attr_available,
                    egui::Checkbox::new(&mut show_attr_overlay, "Show CGB attr overlay"),
                );
                ui.add_enabled(
                    cgb_attr_available,
                    egui::Checkbox::new(&mut render_cgb_colors, "Render CGB colors"),
                );
                if !cgb_attr_available {
                    ui.label("(CGB attr data unavailable)");
                }
            });
            ui.ctx()
                .data_mut(|d| d.insert_persisted(attr_overlay_id, show_attr_overlay));
            ui.ctx()
                .data_mut(|d| d.insert_persisted(render_cgb_color_id, render_cgb_colors));

            let tile_map_base = if use_window_map {
                win_tile_map_base
            } else {
                bg_tile_map_base
            };
            let tile_data_unsigned = ppu.lcdc & 0x10 != 0;
            let mut image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);

            for y in 0..height {
                let tile_row = y / 8;
                let line_in_tile = y % 8;
                for x in 0..width {
                    let tile_col = x / 8;
                    let tile_map_addr = tile_map_base + tile_row * 32 + tile_col;
                    let tile_index = vram.get(tile_map_addr).copied().unwrap_or(0);
                    let raw_attr = if cgb_attr_available {
                        vram.get(0x2000 + tile_map_addr).copied().unwrap_or(0)
                    } else {
                        0
                    };
                    let attr = decode_tile_attr(raw_attr);

                    let tile_data_addr = tile_data_address(tile_index, tile_data_unsigned);
                    let source_line = if render_cgb_colors && attr.flip_y {
                        7 - line_in_tile
                    } else {
                        line_in_tile
                    };
                    let pixel_in_tile = x % 8;
                    let source_pixel = if render_cgb_colors && attr.flip_x {
                        7 - pixel_in_tile
                    } else {
                        pixel_in_tile
                    };
                    let banked_tile_addr = if render_cgb_colors {
                        tile_data_addr + (attr.vram_bank as usize) * 0x2000
                    } else {
                        tile_data_addr
                    };
                    let color_id = decode_tile_pixel(vram, banked_tile_addr, source_line, source_pixel);
                    let rgba = if render_cgb_colors {
                        cgb_palette_rgba(bg_palette_ram, attr.palette, color_id)
                    } else {
                        apply_palette(ppu.bgp, color_id)
                    };
                    let mut final_rgba = rgba;

                    if show_attr_overlay && cgb_attr_available {
                        if attr.priority {
                            final_rgba = [
                                (u16::from(final_rgba[0]) / 2) as u8,
                                (u16::from(final_rgba[1]) / 2) as u8,
                                final_rgba[2],
                                final_rgba[3],
                            ];
                        }
                    }

                    image[(x, y)] =
                        egui::Color32::from_rgba_unmultiplied(
                            final_rgba[0],
                            final_rgba[1],
                            final_rgba[2],
                            final_rgba[3],
                        );
                }
            }

            let texture = ui
                .ctx()
                .load_texture("tilemap_viewer", image, egui::TextureOptions::NEAREST);
            let display_size = egui::vec2((width as f32) * 1.5, (height as f32) * 1.5);
            egui::ScrollArea::both().show(ui, |ui| {
                let response = ui.image((texture.id(), display_size));
                if cgb_attr_available {
                    if let Some(pointer_pos) = response.hover_pos() {
                        let rel_x = ((pointer_pos.x - response.rect.min.x) * (width as f32)
                            / response.rect.width())
                            .floor();
                        let rel_y = ((pointer_pos.y - response.rect.min.y) * (height as f32)
                            / response.rect.height())
                            .floor();

                        if rel_x >= 0.0 && rel_y >= 0.0 {
                            let px = rel_x as usize;
                            let py = rel_y as usize;
                            if px < width && py < height {
                                let tile_row = py / 8;
                                let tile_col = px / 8;
                                let tile_map_addr = tile_map_base + tile_row * 32 + tile_col;
                                let tile_index = vram.get(tile_map_addr).copied().unwrap_or(0);
                                let raw_attr = vram.get(0x2000 + tile_map_addr).copied().unwrap_or(0);
                                let attr = decode_tile_attr(raw_attr);
                                ui.separator();
                                ui.monospace(format!(
                                    "Tile ({:3}, {:3}) map:{:04X} idx:{:02X} attr:{:02X} pal:{} bank:{} fx:{} fy:{} prio:{}",
                                    px,
                                    py,
                                    0x8000 + tile_map_addr,
                                    tile_index,
                                    raw_attr,
                                    attr.palette,
                                    attr.vram_bank,
                                    if attr.flip_x { 1 } else { 0 },
                                    if attr.flip_y { 1 } else { 0 },
                                    if attr.priority { 1 } else { 0 },
                                ));
                            }
                        }
                    }
                }
            });
        });
}

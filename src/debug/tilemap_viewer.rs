use crate::debug::{PpuSnapshot, TilemapViewerState};
use crate::hardware::ppu::{LCDC_BG_TILEMAP, LCDC_TILE_DATA, LCDC_WINDOW_TILEMAP, apply_palette, cgb_palette_rgba, decode_tile_pixel, tile_data_address};
use crate::settings::ColorCorrection;

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

pub(super) fn draw_tilemap_viewer_content(
    ui: &mut egui::Ui,
    vram: &[u8],
    ppu: PpuSnapshot,
    cgb_mode: bool,
    bg_palette_ram: &[u8; 64],
    color_correction: ColorCorrection,
    color_correction_matrix: [f32; 9],
    window_state: &mut TilemapViewerState,
) {
    let width = 256usize;
    let height = 256usize;
    let bg_tile_map_base = if ppu.lcdc & LCDC_BG_TILEMAP != 0 { 0x1C00 } else { 0x1800 };
    let win_tile_map_base = if ppu.lcdc & LCDC_WINDOW_TILEMAP != 0 { 0x1C00 } else { 0x1800 };

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

    let show_viewport_id = ui.make_persistent_id("tilemap_viewer_show_viewport");
    let mut show_viewport = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<bool>(show_viewport_id))
        .unwrap_or(true);
    ui.checkbox(&mut show_viewport, "Show screen viewport");
    ui.ctx()
        .data_mut(|d| d.insert_persisted(show_viewport_id, show_viewport));
    ui.ctx()
        .data_mut(|d| d.insert_persisted(attr_overlay_id, show_attr_overlay));
    ui.ctx()
        .data_mut(|d| d.insert_persisted(render_cgb_color_id, render_cgb_colors));

    let tile_map_base = if use_window_map {
        win_tile_map_base
    } else {
        bg_tile_map_base
    };
    let tile_data_unsigned = ppu.lcdc & LCDC_TILE_DATA != 0;

    let options_changed = window_state.last_use_window_map != Some(use_window_map)
        || window_state.last_show_attr_overlay != Some(show_attr_overlay)
        || window_state.last_render_cgb_colors != Some(render_cgb_colors);
    if options_changed {
        window_state.vram_dirty = true;
        window_state.last_use_window_map = Some(use_window_map);
        window_state.last_show_attr_overlay = Some(show_attr_overlay);
        window_state.last_render_cgb_colors = Some(render_cgb_colors);
    }

    if window_state.image.size != [width, height] {
        window_state.image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        window_state.vram_dirty = true;
    }

    if window_state.vram_dirty {
        render_tilemap_into_image(
            &mut window_state.image,
            vram,
            ppu,
            cgb_mode,
            bg_palette_ram,
            tile_map_base,
            tile_data_unsigned,
            cgb_attr_available,
            render_cgb_colors,
            show_attr_overlay,
            color_correction,
            color_correction_matrix,
        );
        window_state.vram_dirty = false;
    }

    let texture = window_state.texture.get_or_insert_with(|| {
        ui.ctx().load_texture(
            "tilemap_viewer",
            window_state.image.clone(),
            egui::TextureOptions::NEAREST,
        )
    });
    texture.set(window_state.image.clone(), egui::TextureOptions::NEAREST);

    let display_size = egui::vec2((width as f32) * 1.5, (height as f32) * 1.5);
    ui.horizontal(|ui| {
        super::export::export_png_button(ui, "tilemap.png", &window_state.image);
    });
    egui::ScrollArea::both().show(ui, |ui| {
        let response = ui.image((texture.id(), display_size));

        if show_viewport {
            let scale_x = response.rect.width() / width as f32;
            let scale_y = response.rect.height() / height as f32;
            let origin = response.rect.min;
            let painter = ui.painter_at(response.rect);

            if !use_window_map {
                let scx = ppu.scx as f32;
                let scy = ppu.scy as f32;
                draw_wrapped_viewport_rect(
                    &painter, origin, scale_x, scale_y,
                    scx, scy, 160.0, 144.0, 256.0, 256.0,
                    egui::Color32::from_rgba_unmultiplied(0, 255, 0, 200),
                );
            } else {
                let wx = (ppu.wx as f32 - 7.0).max(0.0);
                let wy = ppu.wy as f32;
                let view_w = (160.0 - wx).clamp(0.0, 160.0);
                let view_h = (144.0 - wy).clamp(0.0, 144.0);
                if view_w > 0.0 && view_h > 0.0 {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(origin.x, origin.y),
                        egui::vec2(view_w * scale_x, view_h * scale_y),
                    );
                    painter.rect_stroke(
                        rect,
                        0.0,
                        egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(255, 165, 0, 200)),
                        egui::StrokeKind::Outside,
                    );
                }
            }
        }

        if cgb_attr_available
            && let Some(pointer_pos) = response.hover_pos() {
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
    });
}

fn draw_wrapped_viewport_rect(
    painter: &egui::Painter,
    origin: egui::Pos2,
    scale_x: f32,
    scale_y: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    map_w: f32,
    map_h: f32,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(2.0, color);
    let x2 = x + w;
    let y2 = y + h;
    let wraps_x = x2 > map_w;
    let wraps_y = y2 > map_h;

    let mut rects = Vec::with_capacity(4);
    if !wraps_x && !wraps_y {
        rects.push((x, y, w, h));
    } else if wraps_x && !wraps_y {
        let w1 = map_w - x;
        let w2 = x2 - map_w;
        rects.push((x, y, w1, h));
        rects.push((0.0, y, w2, h));
    } else if !wraps_x && wraps_y {
        let h1 = map_h - y;
        let h2 = y2 - map_h;
        rects.push((x, y, w, h1));
        rects.push((x, 0.0, w, h2));
    } else {
        let w1 = map_w - x;
        let w2 = x2 - map_w;
        let h1 = map_h - y;
        let h2 = y2 - map_h;
        rects.push((x, y, w1, h1));
        rects.push((0.0, y, w2, h1));
        rects.push((x, 0.0, w1, h2));
        rects.push((0.0, 0.0, w2, h2));
    }

    for (rx, ry, rw, rh) in rects {
        let rect = egui::Rect::from_min_size(
            egui::pos2(origin.x + rx * scale_x, origin.y + ry * scale_y),
            egui::vec2(rw * scale_x, rh * scale_y),
        );
        painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Outside);
    }
}

fn render_tilemap_into_image(
    image: &mut egui::ColorImage,
    vram: &[u8],
    ppu: PpuSnapshot,
    _cgb_mode: bool,
    bg_palette_ram: &[u8; 64],
    tile_map_base: usize,
    tile_data_unsigned: bool,
    cgb_attr_available: bool,
    render_cgb_colors: bool,
    show_attr_overlay: bool,
    color_correction: ColorCorrection,
    color_correction_matrix: [f32; 9],
) {
    let width = image.size[0];
    let height = image.size[1];
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
                cgb_palette_rgba(
                    bg_palette_ram,
                    attr.palette,
                    color_id,
                    color_correction,
                    color_correction_matrix,
                )
            } else {
                apply_palette(ppu.bgp, color_id)
            };
            let mut final_rgba = rgba;

            if show_attr_overlay && cgb_attr_available && attr.priority {
                final_rgba = [
                    (u16::from(final_rgba[0]) / 2) as u8,
                    (u16::from(final_rgba[1]) / 2) as u8,
                    final_rgba[2],
                    final_rgba[3],
                ];
            }

            image[(x, y)] = egui::Color32::from_rgba_unmultiplied(
                final_rgba[0],
                final_rgba[1],
                final_rgba[2],
                final_rgba[3],
            );
        }
    }
}

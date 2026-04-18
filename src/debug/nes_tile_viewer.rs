use crate::debug::TileViewerState;
use crate::debug::common::nes_palette_rgba;
use crate::debug::types::NesGraphicsData;

pub(super) fn decode_nes_tile_pixel(chr: &[u8], tile_addr: usize, row: usize, col: usize) -> u8 {
    let lo = chr.get(tile_addr + row).copied().unwrap_or(0);
    let hi = chr.get(tile_addr + 8 + row).copied().unwrap_or(0);
    let bit = 7 - col;
    let p0 = (lo >> bit) & 1;
    let p1 = (hi >> bit) & 1;
    (p1 << 1) | p0
}

pub(super) fn draw_nes_tile_viewer_content(
    ui: &mut egui::Ui,
    gfx: &NesGraphicsData,
    window_state: &mut TileViewerState,
) {
    let palette_id = ui.make_persistent_id("nes_tile_viewer_palette");
    let mut palette_index: u8 = ui
        .ctx()
        .data_mut(|d| d.get_persisted(palette_id))
        .unwrap_or(0)
        .min(7);

    let obj_mode_id = ui.make_persistent_id("nes_tile_viewer_obj");
    let mut use_obj = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<bool>(obj_mode_id))
        .unwrap_or(false);

    ui.horizontal(|ui| {
        ui.checkbox(&mut use_obj, "OBJ palettes");
        ui.label("Palette:");
        for idx in 0u8..4 {
            ui.selectable_value(&mut palette_index, idx, format!("{idx}"));
        }
    });

    ui.ctx()
        .data_mut(|d| d.insert_persisted(palette_id, palette_index));
    ui.ctx()
        .data_mut(|d| d.insert_persisted(obj_mode_id, use_obj));

    let width = 256usize;
    let height = 128usize;

    let options_changed = window_state.last_use_cgb_colors != Some(use_obj)
        || window_state.last_cgb_palette_index != Some(palette_index);
    if options_changed {
        window_state.tracker.vram_dirty = true;
        window_state.last_use_cgb_colors = Some(use_obj);
        window_state.last_cgb_palette_index = Some(palette_index);
    }

    if window_state.image.size != [width, height] {
        window_state.image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        window_state.tracker.vram_dirty = true;
    }

    if window_state.tracker.vram_dirty {
        render_nes_pattern_tables(
            &mut window_state.image,
            &gfx.chr_data,
            &gfx.palette_ram,
            gfx.palette_mode,
            palette_index,
            use_obj,
        );
        window_state.tracker.vram_dirty = false;
    }

    super::common::show_viewer_texture(
        ui,
        &mut window_state.texture,
        &window_state.image,
        "nes_tile_viewer",
        "nes_tiles.png",
        2.0,
    );

    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Pattern Table 0 ($0000)");
        ui.add_space(width as f32 - 100.0);
        ui.label("Pattern Table 1 ($1000)");
    });
}

fn render_nes_pattern_tables(
    image: &mut egui::ColorImage,
    chr_data: &[u8],
    palette_ram: &[u8; 32],
    palette_mode: zeff_nes_core::hardware::ppu::NesPaletteMode,
    palette_index: u8,
    use_obj: bool,
) {
    let effective_palette = if use_obj {
        palette_index + 4
    } else {
        palette_index
    };

    for table in 0..2usize {
        let table_base = table * 0x1000;
        let x_offset = table * 128;
        for tile in 0..256usize {
            let tile_x = tile % 16;
            let tile_y = tile / 16;
            let tile_addr = table_base + tile * 16;
            for row in 0..8usize {
                for col in 0..8usize {
                    let color_id = decode_nes_tile_pixel(chr_data, tile_addr, row, col);
                    let rgba =
                        nes_palette_rgba(palette_ram, effective_palette, color_id, palette_mode);
                    let px = x_offset + tile_x * 8 + col;
                    let py = tile_y * 8 + row;
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

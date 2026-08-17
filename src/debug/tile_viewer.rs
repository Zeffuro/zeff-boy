use crate::debug::types::GbGraphicsData;
use crate::debug::{TileViewerPlatform, TileViewerRequest, TileViewerState};
use zeff_gb_core::hardware::ppu::{
    apply_dmg_palette, cgb_palette_rgba, correct_color, decode_tile_pixel,
};

pub(super) fn draw_tile_viewer_content(
    ui: &mut egui::Ui,
    gfx: &GbGraphicsData,
    bgp: u8,
    window_state: &mut TileViewerState,
) {
    let vram = &gfx.vram;
    let cgb_mode = gfx.cgb_mode;
    let bank_select_id = ui.make_persistent_id("tile_viewer_vram_bank");
    let mut vram_bank = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<usize>(bank_select_id))
        .unwrap_or(0);
    let max_bank = if vram.len() >= 0x4000 { 1 } else { 0 };
    if vram_bank > max_bank {
        vram_bank = max_bank;
    }

    ui.horizontal(|ui| {
        ui.label("VRAM bank:");
        ui.selectable_value(&mut vram_bank, 0, "0");
        if max_bank >= 1 {
            ui.selectable_value(&mut vram_bank, 1, "1");
        }
    });
    ui.ctx()
        .data_mut(|d| d.insert_persisted(bank_select_id, vram_bank));

    let color_mode_id = ui.make_persistent_id("tile_viewer_color_mode");
    let mut use_cgb_colors = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<bool>(color_mode_id))
        .unwrap_or(cgb_mode);
    if !cgb_mode {
        use_cgb_colors = false;
    }

    let cgb_obj_mode_id = ui.make_persistent_id("tile_viewer_cgb_obj_mode");
    let mut use_obj_palette = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<bool>(cgb_obj_mode_id))
        .unwrap_or(false);

    let cgb_palette_index_id = ui.make_persistent_id("tile_viewer_cgb_palette_index");
    let mut cgb_palette_index = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<u8>(cgb_palette_index_id))
        .unwrap_or(0)
        .min(7);

    if let Some(TileViewerRequest::Gb {
        tile,
        bank,
        obj_palette,
        palette,
    }) = window_state.pending.take()
    {
        vram_bank = bank.min(max_bank);
        use_obj_palette = obj_palette;
        cgb_palette_index = palette.min(7);
        window_state.select(TileViewerPlatform::Gb, tile.min(383));
    }

    ui.horizontal(|ui| {
        ui.add_enabled(
            cgb_mode,
            egui::Checkbox::new(&mut use_cgb_colors, "Use CGB colors"),
        );
        if use_cgb_colors {
            ui.checkbox(&mut use_obj_palette, "OBJ palettes");
            ui.label("Palette:");
            for index in 0u8..8 {
                ui.selectable_value(&mut cgb_palette_index, index, format!("{}", index));
            }
        }
    });

    ui.ctx()
        .data_mut(|d| d.insert_persisted(color_mode_id, use_cgb_colors));
    ui.ctx()
        .data_mut(|d| d.insert_persisted(cgb_obj_mode_id, use_obj_palette));
    ui.ctx()
        .data_mut(|d| d.insert_persisted(cgb_palette_index_id, cgb_palette_index));

    let width = 16 * 8;
    let height = 24 * 8;

    let options_changed = window_state.last_vram_bank != Some(vram_bank)
        || window_state.last_use_cgb_colors != Some(use_cgb_colors)
        || window_state.last_use_obj_palette != Some(use_obj_palette)
        || window_state.last_cgb_palette_index != Some(cgb_palette_index);
    if options_changed {
        window_state.tracker.vram_dirty = true;
        window_state.last_vram_bank = Some(vram_bank);
        window_state.last_use_cgb_colors = Some(use_cgb_colors);
        window_state.last_use_obj_palette = Some(use_obj_palette);
        window_state.last_cgb_palette_index = Some(cgb_palette_index);
    }

    if window_state.image.size != [width, height] {
        window_state.image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        window_state.tracker.vram_dirty = true;
    }

    let bank_base = vram_bank * 0x2000;
    if window_state.tracker.vram_dirty {
        render_tile_viewer_into_image(
            &mut window_state.image,
            gfx,
            &TileRenderOptions {
                bgp,
                use_cgb_colors,
                use_obj_palette,
                cgb_palette_index,
                bank_base,
            },
        );
        window_state.tracker.vram_dirty = false;
    }

    let response = super::common::show_viewer_texture(
        ui,
        &mut window_state.texture,
        &window_state.image,
        "tile_viewer",
        "tiles.png",
        2.0,
    );
    if response.clicked()
        && let Some((x, y)) = super::common::hover_pixel_coords(&response, width, height)
    {
        window_state.select(TileViewerPlatform::Gb, (y / 8) * 16 + x / 8);
    }
    if window_state.selected_platform == Some(TileViewerPlatform::Gb)
        && let Some(tile) = window_state.selected
    {
        super::common::draw_grid_selection(ui, &response, 16, 24, tile);
        ui.horizontal_wrapped(|ui| {
            ui.monospace(format!("Tile ${tile:03X}"));
            ui.monospace(format!("VRAM bank {vram_bank}"));
            ui.monospace(format!("address ${:04X}", 0x8000 + tile * 16));
        });
    }
}

struct TileRenderOptions {
    bgp: u8,
    use_cgb_colors: bool,
    use_obj_palette: bool,
    cgb_palette_index: u8,
    bank_base: usize,
}

fn render_tile_viewer_into_image(
    image: &mut egui::ColorImage,
    gfx: &GbGraphicsData,
    opts: &TileRenderOptions,
) {
    let vram = &gfx.vram;
    let bg_palette_ram = &gfx.bg_palette_ram;
    let obj_palette_ram = &gfx.obj_palette_ram;
    let color_correction = gfx.color_correction;
    let color_correction_matrix = gfx.color_correction_matrix;
    for tile in 0..384usize {
        let tile_x = tile % 16;
        let tile_y = tile / 16;
        let tile_addr = opts.bank_base + tile * 16;

        for y in 0..8usize {
            for x in 0..8usize {
                let color_id = decode_tile_pixel(vram, tile_addr, y, x);
                let rgba = if opts.use_cgb_colors {
                    let palette_ram = if opts.use_obj_palette {
                        obj_palette_ram
                    } else {
                        bg_palette_ram
                    };
                    correct_color(
                        cgb_palette_rgba(palette_ram, opts.cgb_palette_index, color_id),
                        color_correction,
                        color_correction_matrix,
                    )
                } else {
                    apply_dmg_palette(gfx.dmg_palette_preset, opts.bgp, color_id)
                };
                let px = tile_x * 8 + x;
                let py = tile_y * 8 + y;
                image[(px, py)] =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

use crate::debug::DebugWindowState;
use crate::hardware::ppu::{apply_palette, cgb_palette_rgba, decode_tile_pixel};


pub(super) fn draw_tile_viewer_content(
    ui: &mut egui::Ui,
    vram: &[u8],
    bgp: u8,
    cgb_mode: bool,
    bg_palette_ram: &[u8; 64],
    obj_palette_ram: &[u8; 64],
    window_state: &mut DebugWindowState,
) {
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

    let options_changed = window_state.tile_viewer_last_vram_bank != Some(vram_bank)
        || window_state.tile_viewer_last_use_cgb_colors != Some(use_cgb_colors)
        || window_state.tile_viewer_last_use_obj_palette != Some(use_obj_palette)
        || window_state.tile_viewer_last_cgb_palette_index != Some(cgb_palette_index);
    if options_changed {
        window_state.tile_viewer_vram_dirty = true;
        window_state.tile_viewer_last_vram_bank = Some(vram_bank);
        window_state.tile_viewer_last_use_cgb_colors = Some(use_cgb_colors);
        window_state.tile_viewer_last_use_obj_palette = Some(use_obj_palette);
        window_state.tile_viewer_last_cgb_palette_index = Some(cgb_palette_index);
    }

    if window_state.tile_viewer_image.size != [width, height] {
        window_state.tile_viewer_image =
            egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        window_state.tile_viewer_vram_dirty = true;
    }

    let bank_base = vram_bank * 0x2000;
    if window_state.tile_viewer_vram_dirty {
        render_tile_viewer_into_image(
            &mut window_state.tile_viewer_image,
            vram,
            bgp,
            use_cgb_colors,
            use_obj_palette,
            cgb_palette_index,
            bg_palette_ram,
            obj_palette_ram,
            bank_base,
        );
        window_state.tile_viewer_vram_dirty = false;
    }

    let texture = window_state.tile_viewer_texture.get_or_insert_with(|| {
        ui.ctx().load_texture(
            "tile_viewer",
            window_state.tile_viewer_image.clone(),
            egui::TextureOptions::NEAREST,
        )
    });
    texture.set(
        window_state.tile_viewer_image.clone(),
        egui::TextureOptions::NEAREST,
    );

    let display_size = egui::vec2((width as f32) * 2.0, (height as f32) * 2.0);
    ui.horizontal(|ui| {
        super::export::export_png_button(
            ui,
            "tiles.png",
            &window_state.tile_viewer_image,
        );
    });
    egui::ScrollArea::both().show(ui, |ui| {
        ui.image((texture.id(), display_size));
    });
}

fn render_tile_viewer_into_image(
    image: &mut egui::ColorImage,
    vram: &[u8],
    bgp: u8,
    use_cgb_colors: bool,
    use_obj_palette: bool,
    cgb_palette_index: u8,
    bg_palette_ram: &[u8; 64],
    obj_palette_ram: &[u8; 64],
    bank_base: usize,
) {
    for tile in 0..384usize {
        let tile_x = tile % 16;
        let tile_y = tile / 16;
        let tile_addr = bank_base + tile * 16;

        for y in 0..8usize {
            for x in 0..8usize {
                let color_id = decode_tile_pixel(vram, tile_addr, y, x);
                let rgba = if use_cgb_colors {
                    let palette_ram = if use_obj_palette {
                        obj_palette_ram
                    } else {
                        bg_palette_ram
                    };
                    cgb_palette_rgba(palette_ram, cgb_palette_index, color_id)
                } else {
                    apply_palette(bgp, color_id)
                };
                let px = tile_x * 8 + x;
                let py = tile_y * 8 + y;
                image[(px, py)] =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

use crate::debug::common::sega8_palette_rgba;
use crate::debug::tms9918_graphics::{
    TMS_TILE_COLUMNS, TMS_TILE_COUNT, TMS_TILE_SIZE, atlas_page_count, color_rgba, is_tms,
    mode_label, pattern_address, tile_color,
};
use crate::debug::types::Sega8GraphicsData;
use crate::debug::{TileViewerPlatform, TileViewerRequest, TileViewerState};

const COLUMNS: usize = 16;
const TILE_COUNT: usize = 512;

pub(super) fn draw_sega8_tile_viewer_content(
    ui: &mut egui::Ui,
    gfx: &Sega8GraphicsData,
    state: &mut TileViewerState,
) {
    if !gfx.mode4.enabled {
        draw_tms_tile_viewer_content(ui, gfx, state);
        return;
    }

    let palette_id = ui.make_persistent_id("sega8_tile_obj_palette");
    let mut use_obj = ui
        .ctx()
        .data_mut(|data| data.get_persisted::<bool>(palette_id))
        .unwrap_or(false);
    if let Some(TileViewerRequest::Sega8 { tile, .. }) = state.pending.take() {
        use_obj = true;
        state.select(TileViewerPlatform::Sega8, tile.min(TILE_COUNT - 1));
    }
    ui.checkbox(&mut use_obj, "OBJ palette");
    ui.ctx()
        .data_mut(|data| data.insert_persisted(palette_id, use_obj));

    let width = COLUMNS * 8;
    let rows = TILE_COUNT.div_ceil(COLUMNS);
    let height = rows * 8;
    let signature = u64::from(crc32fast::hash(&gfx.vram))
        ^ u64::from(crc32fast::hash(&gfx.cram)).rotate_left(17)
        ^ u64::from(use_obj);
    if state.tracker.last_vram_signature != signature || state.image.size != [width, height] {
        state.tracker.last_vram_signature = signature;
        state.image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        render_tiles(&mut state.image, gfx, use_obj);
    }

    let response = super::common::show_viewer_texture(
        ui,
        &mut state.texture,
        &state.image,
        "sega8_tile_viewer",
        "sega8_tiles.png",
        2.0,
    );
    if response.clicked()
        && let Some((x, y)) = super::common::hover_pixel_coords(&response, width, height)
    {
        state.select(TileViewerPlatform::Sega8, (y / 8) * COLUMNS + x / 8);
    }
    if state.selected_platform == Some(TileViewerPlatform::Sega8)
        && let Some(tile) = state.selected
    {
        super::common::draw_grid_selection(ui, &response, COLUMNS, rows, tile);
        ui.horizontal_wrapped(|ui| {
            ui.monospace(format!("Tile ${tile:03X}"));
            ui.monospace(format!("VRAM ${:04X}", tile * 32));
            if tile >= gfx.mode4.sprite_pattern_base / 32 {
                ui.weak("sprite pattern range");
            }
        });
    }
}

fn draw_tms_tile_viewer_content(
    ui: &mut egui::Ui,
    gfx: &Sega8GraphicsData,
    state: &mut TileViewerState,
) {
    debug_assert!(is_tms(gfx));
    let page_id = ui.make_persistent_id("tms9918_tile_page");
    let mut page = ui
        .ctx()
        .data_mut(|data| data.get_persisted::<usize>(page_id))
        .unwrap_or(0)
        .min(atlas_page_count(&gfx.tms9918).saturating_sub(1));
    if let Some(TileViewerRequest::Sega8 { tile, tms_section }) = state.pending.take() {
        page = tms_section
            .unwrap_or(page)
            .min(atlas_page_count(&gfx.tms9918).saturating_sub(1));
        state.select(TileViewerPlatform::Sega8, tile.min(TMS_TILE_COUNT - 1));
    }
    if atlas_page_count(&gfx.tms9918) > 1 {
        ui.horizontal(|ui| {
            ui.label("Pattern page:");
            for section in 0..atlas_page_count(&gfx.tms9918) {
                ui.selectable_value(&mut page, section, format!("{}", section));
            }
        });
    }
    ui.ctx()
        .data_mut(|data| data.insert_persisted(page_id, page));
    ui.monospace(format!(
        "{}  pattern ${:04X}  color ${:04X}",
        mode_label(gfx.tms9918.mode),
        gfx.tms9918.pattern_table_base,
        gfx.tms9918.color_table_base
    ));

    let width = TMS_TILE_COLUMNS * TMS_TILE_SIZE;
    let height = TMS_TILE_COUNT.div_ceil(TMS_TILE_COLUMNS) * TMS_TILE_SIZE;
    let signature = u64::from(crc32fast::hash(&gfx.vram))
        ^ u64::from(crc32fast::hash(&gfx.cram)).rotate_left(17)
        ^ u64::from(crc32fast::hash(&gfx.registers)).rotate_left(31)
        ^ (page as u64).rotate_left(47);
    if state.tracker.last_vram_signature != signature || state.image.size != [width, height] {
        state.tracker.last_vram_signature = signature;
        state.image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        render_tms_tiles(&mut state.image, gfx, page);
    }

    let response = super::common::show_viewer_texture(
        ui,
        &mut state.texture,
        &state.image,
        "tms9918_tile_viewer",
        "tms9918_tiles.png",
        2.0,
    );
    if response.clicked()
        && let Some((x, y)) = super::common::hover_pixel_coords(&response, width, height)
    {
        state.select(
            TileViewerPlatform::Sega8,
            (y / TMS_TILE_SIZE) * TMS_TILE_COLUMNS + x / TMS_TILE_SIZE,
        );
    }
    if state.selected_platform == Some(TileViewerPlatform::Sega8)
        && let Some(tile) = state.selected
    {
        super::common::draw_grid_selection(ui, &response, TMS_TILE_COLUMNS, 16, tile);
        ui.horizontal_wrapped(|ui| {
            ui.monospace(format!("Pattern ${tile:02X}"));
            ui.monospace(format!(
                "VRAM ${:04X}",
                pattern_address(&gfx.tms9918, tile, page, 0)
            ));
            if let Some(address) =
                crate::debug::tms9918_graphics::color_address(&gfx.tms9918, tile, page, 0)
            {
                ui.monospace(format!("color ${address:04X}"));
            }
        });
    }
}

fn render_tiles(image: &mut egui::ColorImage, gfx: &Sega8GraphicsData, use_obj: bool) {
    for tile in 0..TILE_COUNT {
        let tile_x = tile % COLUMNS;
        let tile_y = tile / COLUMNS;
        for y in 0..8 {
            for x in 0..8 {
                let color = pattern_color(&gfx.vram, tile, x, y);
                let rgba =
                    sega8_palette_rgba(gfx.system, &gfx.cram, color + if use_obj { 16 } else { 0 });
                image[(tile_x * 8 + x, tile_y * 8 + y)] =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

fn render_tms_tiles(image: &mut egui::ColorImage, gfx: &Sega8GraphicsData, page: usize) {
    for tile in 0..TMS_TILE_COUNT {
        let tile_x = tile % TMS_TILE_COLUMNS;
        let tile_y = tile / TMS_TILE_COLUMNS;
        for y in 0..TMS_TILE_SIZE {
            for x in 0..TMS_TILE_SIZE {
                let color = tile_color(&gfx.vram, &gfx.tms9918, tile, page, x, y);
                let rgba = color_rgba(gfx, color);
                image[(tile_x * TMS_TILE_SIZE + x, tile_y * TMS_TILE_SIZE + y)] =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

pub(super) fn pattern_color(vram: &[u8], tile: usize, x: usize, y: usize) -> usize {
    let base = tile * 32 + y * 4;
    let bit = 0x80 >> x;
    (0..4).fold(0, |color, plane| {
        color | usize::from(vram.get(base + plane).copied().unwrap_or(0) & bit != 0) << plane
    })
}

#[cfg(test)]
mod tests {
    use super::pattern_color;

    #[test]
    fn decodes_mode4_bitplanes() {
        let mut vram = vec![0; 0x4000];
        vram[0] = 0x80;
        vram[2] = 0x80;

        assert_eq!(pattern_color(&vram, 0, 0, 0), 5);
        assert_eq!(pattern_color(&vram, 0, 1, 0), 0);
    }
}

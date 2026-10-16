use crate::debug::types::MdGraphicsData;
use crate::debug::{TileViewerPlatform, TileViewerState};

const COLUMNS: usize = 16;
const WIDTH: usize = COLUMNS * 8;

pub(super) fn draw_md_tile_viewer_content(
    ui: &mut egui::Ui,
    gfx: &MdGraphicsData,
    state: &mut TileViewerState,
) {
    let Some(tile_height) = gfx.tile_height else {
        ui.weak("Tile inspection requires known 64 KiB Mode 5 progressive or interlace mode 2 settings.");
        return;
    };
    let palette_id = ui.make_persistent_id("md_tile_palette");
    let mut palette = ui
        .ctx()
        .data_mut(|data| data.get_persisted::<usize>(palette_id))
        .unwrap_or(0)
        .min(3);
    ui.horizontal(|ui| {
        ui.label("Palette:");
        for index in 0..4 {
            ui.selectable_value(&mut palette, index, index.to_string());
        }
    });
    ui.ctx()
        .data_mut(|data| data.insert_persisted(palette_id, palette));
    ui.weak(format!(
        "8 × {tile_height} patterns · normal intensity · color 0 shown opaque"
    ));
    refresh_image(state, gfx, palette);
    let count = tile_count(tile_height);
    let rows = count.div_ceil(COLUMNS);
    let height = rows * tile_height;
    let response = super::common::show_viewer_texture(
        ui,
        &mut state.texture,
        &state.image,
        "md_tile_viewer",
        "md_tiles.png",
        2.0,
    );
    if response.clicked()
        && let Some((x, y)) = super::common::hover_pixel_coords(&response, WIDTH, height)
    {
        state.select(TileViewerPlatform::Md, (y / tile_height) * COLUMNS + x / 8);
    }
    if state.selected_platform == Some(TileViewerPlatform::Md)
        && let Some(tile) = state.selected
    {
        let tile = tile.min(count - 1);
        state.select(TileViewerPlatform::Md, tile);
        super::common::draw_grid_selection(ui, &response, COLUMNS, rows, tile);
        ui.monospace(format!(
            "Pattern ${tile:03X}  VRAM ${:04X}",
            tile * tile_height * 4
        ));
    }
}

fn tile_count(tile_height: usize) -> usize {
    0x10000 / (tile_height * 4)
}

fn refresh_image(state: &mut TileViewerState, gfx: &MdGraphicsData, palette: usize) {
    let Some(tile_height) = gfx.tile_height else {
        return;
    };
    let height = tile_count(tile_height).div_ceil(COLUMNS) * tile_height;
    let palette_bytes = gfx.palette.as_flattened();
    let signature = u64::from(crc32fast::hash(&gfx.vram))
        ^ u64::from(crc32fast::hash(palette_bytes)).rotate_left(17)
        ^ (palette as u64).rotate_left(41)
        ^ (tile_height as u64).rotate_left(49);
    if state.tracker.vram_dirty
        || state.tracker.last_vram_signature != signature
        || state.image.size != [WIDTH, height]
    {
        state.tracker.vram_dirty = false;
        state.tracker.last_vram_signature = signature;
        state.image = egui::ColorImage::filled([WIDTH, height], egui::Color32::BLACK);
        render_tiles(&mut state.image, gfx, palette, tile_height);
    }
}

fn render_tiles(
    image: &mut egui::ColorImage,
    gfx: &MdGraphicsData,
    palette: usize,
    tile_height: usize,
) {
    for tile in 0..tile_count(tile_height) {
        for y in 0..tile_height {
            for x in 0..8 {
                let color = pattern_color(&gfx.vram, tile, tile_height, x, y);
                let rgba = gfx.palette[palette * 16 + color];
                image[(tile % COLUMNS * 8 + x, tile / COLUMNS * tile_height + y)] =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

fn pattern_color(vram: &[u8], tile: usize, tile_height: usize, x: usize, y: usize) -> usize {
    let byte = vram
        .get(tile * tile_height * 4 + y * 4 + x / 2)
        .copied()
        .unwrap_or(0);
    usize::from(if x & 1 == 0 { byte >> 4 } else { byte & 15 })
}

#[cfg(test)]
mod tests;

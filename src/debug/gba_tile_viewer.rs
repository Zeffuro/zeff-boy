use crate::debug::TileViewerState;
use crate::debug::types::GbaGraphicsData;

pub(super) fn draw_gba_tile_viewer_content(
    ui: &mut egui::Ui,
    gfx: &GbaGraphicsData,
    window_state: &mut TileViewerState,
) {
    let char_base_id = ui.make_persistent_id("gba_tile_char_base");
    let mut char_base = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<usize>(char_base_id))
        .unwrap_or(0)
        .min(3);
    let color_mode_id = ui.make_persistent_id("gba_tile_color_256");
    let mut color_256 = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<bool>(color_mode_id))
        .unwrap_or(false);
    let obj_tiles_id = ui.make_persistent_id("gba_tile_obj_tiles");
    let mut obj_tiles = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<bool>(obj_tiles_id))
        .unwrap_or(false);
    let palette_id = ui.make_persistent_id("gba_tile_palette");
    let mut palette = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<usize>(palette_id))
        .unwrap_or(0)
        .min(15);

    ui.horizontal(|ui| {
        ui.label("Char base:");
        for i in 0..4usize {
            ui.selectable_value(&mut char_base, i, format!("{}", i));
        }
        ui.checkbox(&mut color_256, "256-color");
        ui.checkbox(&mut obj_tiles, "OBJ tiles");
        if !color_256 {
            ui.label("Palette:");
            for i in 0..16usize {
                ui.selectable_value(&mut palette, i, format!("{}", i));
            }
        }
    });
    ui.ctx().data_mut(|d| {
        d.insert_persisted(char_base_id, char_base);
        d.insert_persisted(color_mode_id, color_256);
        d.insert_persisted(obj_tiles_id, obj_tiles);
        d.insert_persisted(palette_id, palette);
    });

    let width = 16 * 8;
    let height = 24 * 8;
    let signature = fold_bytes(&gfx.vram)
        ^ fold_bytes(&gfx.palette_ram).rotate_left(1)
        ^ fold_bytes(&gfx.oam).rotate_left(2)
        ^ ((char_base as u64) << 8)
        ^ ((color_256 as u64) << 16)
        ^ ((obj_tiles as u64) << 17)
        ^ ((palette as u64) << 24);
    let changed = window_state.tracker.last_vram_signature != signature;
    window_state.tracker.last_vram_signature = signature;
    if changed || window_state.image.size != [width, height] {
        window_state.image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        render_tiles(
            &mut window_state.image,
            gfx,
            char_base,
            color_256,
            obj_tiles,
            palette,
        );
    }

    super::common::show_viewer_texture(
        ui,
        &mut window_state.texture,
        &window_state.image,
        "gba_tile_viewer",
        "gba_tiles.png",
        2.0,
    );
}

fn render_tiles(
    image: &mut egui::ColorImage,
    gfx: &GbaGraphicsData,
    char_base: usize,
    color_256: bool,
    obj_tiles: bool,
    palette: usize,
) {
    let base = if obj_tiles {
        0x10000
    } else {
        char_base * 0x4000
    };
    let tile_stride = if color_256 { 64 } else { 32 };
    for tile in 0..384usize {
        let tile_x = tile % 16;
        let tile_y = tile / 16;
        let tile_addr = base + tile * tile_stride;
        for y in 0..8usize {
            for x in 0..8usize {
                let color_index = if color_256 {
                    gfx.vram.get(tile_addr + y * 8 + x).copied().unwrap_or(0) as usize
                } else {
                    let byte = gfx
                        .vram
                        .get(tile_addr + y * 4 + x / 2)
                        .copied()
                        .unwrap_or(0);
                    let nibble = if x & 1 == 0 { byte & 0x0F } else { byte >> 4 };
                    palette * 16 + nibble as usize
                };
                let rgba = gba_palette_rgba(&gfx.palette_ram, color_index);
                image[(tile_x * 8 + x, tile_y * 8 + y)] =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

pub(crate) fn gba_palette_rgba(palette_ram: &[u8], index: usize) -> [u8; 4] {
    let offset = index * 2;
    let color = u16::from_le_bytes([
        palette_ram.get(offset).copied().unwrap_or(0),
        palette_ram.get(offset + 1).copied().unwrap_or(0),
    ]);
    let r = (color & 0x1F) as u8;
    let g = ((color >> 5) & 0x1F) as u8;
    let b = ((color >> 10) & 0x1F) as u8;
    [expand5(r), expand5(g), expand5(b), 255]
}

fn expand5(v: u8) -> u8 {
    (v << 3) | (v >> 2)
}

fn fold_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0u64, |acc, &b| {
        acc.rotate_left(5).wrapping_add(u64::from(b) + 0x9E37_79B9)
    })
}

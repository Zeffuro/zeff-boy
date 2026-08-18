use crate::debug::tms9918_graphics::{
    TMS_MAP_ROWS, TMS_TILE_SIZE, color_rgba, is_tms, map_cell_at, map_color, mode_label,
    pattern_address,
};
use crate::debug::types::Sega8GraphicsData;
use crate::debug::{
    DebugTab, DebugUiActions, TileViewerRequest, TileViewerState, TilemapViewerState,
};
use zeff_sega8_core::hardware::vdp::Tms9918Mode;

const WIDTH: usize = 256;
const HEIGHT: usize = TMS_MAP_ROWS * TMS_TILE_SIZE;

pub(super) fn draw_tms9918_tilemap_viewer_content(
    ui: &mut egui::Ui,
    gfx: &Sega8GraphicsData,
    state: &mut TilemapViewerState,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    debug_assert!(is_tms(gfx));
    ui.monospace(format!(
        "{}  name ${:04X}  pattern ${:04X}",
        mode_label(gfx.tms9918.mode),
        gfx.tms9918.name_table_base,
        gfx.tms9918.pattern_table_base
    ));
    if matches!(gfx.tms9918.mode, Tms9918Mode::Invalid) {
        ui.weak("The current register combination has no TMS display mode.");
        return;
    }

    let signature = u64::from(crc32fast::hash(&gfx.vram))
        ^ u64::from(crc32fast::hash(&gfx.cram)).rotate_left(17)
        ^ u64::from(crc32fast::hash(&gfx.registers)).rotate_left(31);
    if state.tracker.last_vram_signature != signature || state.image.size != [WIDTH, HEIGHT] {
        state.tracker.last_vram_signature = signature;
        state.image = egui::ColorImage::filled([WIDTH, HEIGHT], egui::Color32::BLACK);
        render_map(&mut state.image, gfx);
    }

    let response = super::common::show_viewer_texture(
        ui,
        &mut state.texture,
        &state.image,
        "tms9918_tilemap_viewer",
        "tms9918_tilemap.png",
        1.5,
    );
    let hovered = super::common::hover_pixel_coords(&response, WIDTH, HEIGHT)
        .and_then(|(x, y)| map_cell_at(&gfx.vram, &gfx.tms9918, x, y));
    if let Some(cell) = hovered {
        let columns = if matches!(gfx.tms9918.mode, Tms9918Mode::Text) {
            40
        } else {
            32
        };
        let selected = cell.row * columns + cell.column;
        if response.clicked() {
            state.selected = Some(selected);
            tiles.pending = Some(TileViewerRequest::Sega8 {
                tile: cell.tile,
                tms_section: Some(cell.section),
            });
            actions.focus_tab = Some(DebugTab::TileViewer);
        }
        if state.selected == Some(selected) {
            draw_selection(ui, &response, gfx.tms9918.mode, cell.column, cell.row);
        }
        let name_address = gfx.tms9918.name_table_base + cell.row * columns + cell.column;
        ui.separator();
        ui.monospace(format!(
            "({:02},{:02}) name ${name_address:04X} pattern ${:02X} VRAM ${:04X}",
            cell.column,
            cell.row,
            cell.tile,
            pattern_address(&gfx.tms9918, cell.tile, cell.section, 0),
        ));
    }
}

fn render_map(image: &mut egui::ColorImage, gfx: &Sega8GraphicsData) {
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let rgba = color_rgba(gfx, map_color(&gfx.vram, &gfx.tms9918, x, y));
            image[(x, y)] =
                egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
        }
    }
}

fn draw_selection(
    ui: &egui::Ui,
    response: &egui::Response,
    mode: Tms9918Mode,
    column: usize,
    row: usize,
) {
    let (x, width) = if matches!(mode, Tms9918Mode::Text) {
        (8 + column * 6, 6)
    } else {
        (column * TMS_TILE_SIZE, TMS_TILE_SIZE)
    };
    let rect = egui::Rect::from_min_size(
        egui::pos2(
            response.rect.min.x + response.rect.width() * x as f32 / WIDTH as f32,
            response.rect.min.y + response.rect.height() * row as f32 / TMS_MAP_ROWS as f32,
        ),
        egui::vec2(
            response.rect.width() * width as f32 / WIDTH as f32,
            response.rect.height() / TMS_MAP_ROWS as f32,
        ),
    );
    ui.painter().rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(
            2.0,
            super::common::color32(super::common::debug_colors(ui).selection),
        ),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_sega8_core::hardware::vdp::Tms9918VdpDebugSnapshot;

    #[test]
    fn text_selection_uses_six_pixel_cells() {
        let rect = selection_geometry(Tms9918Mode::Text, 3, 2);
        assert_eq!(rect, (26, 6, 2));
    }

    fn selection_geometry(mode: Tms9918Mode, column: usize, row: usize) -> (usize, usize, usize) {
        let (x, width) = if matches!(mode, Tms9918Mode::Text) {
            (8 + column * 6, 6)
        } else {
            (column * TMS_TILE_SIZE, TMS_TILE_SIZE)
        };
        (x, width, row)
    }

    #[test]
    fn map_cells_follow_the_name_table_layout() {
        let tms = Tms9918VdpDebugSnapshot {
            mode: Tms9918Mode::GraphicsI,
            name_table_base: 0x1800,
            pattern_table_base: 0,
            color_table_base: 0,
            sprite_attribute_table_base: 0,
            sprite_pattern_table_base: 0,
            backdrop_color: 1,
            text_foreground_color: 15,
            text_background_color: 4,
            sprite_size: 8,
            sprite_magnified: false,
        };
        let mut vram = vec![0; 0x4000];
        vram[0x1800 + 32 + 2] = 0x7E;

        let cell = map_cell_at(&vram, &tms, 16, 8).unwrap();
        assert_eq!(
            (cell.column, cell.row, cell.tile, cell.section),
            (2, 1, 0x7E, 0)
        );
    }
}

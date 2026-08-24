use crate::debug::pce_graphics::{
    TILE_COUNT, TILE_SIZE, color_mode, graphics_signature, palette_rgba, tile_pixel,
};
use crate::debug::{PceGraphicsData, TileViewerPlatform, TileViewerRequest, TileViewerState};

const COLUMNS: usize = 32;

pub(super) fn draw_pce_tile_viewer_content(
    ui: &mut egui::Ui,
    gfx: &PceGraphicsData,
    state: &mut TileViewerState,
) {
    let mut vdc2 = select_vdc(ui, "pce_tile_vdc", gfx.vdc2.is_some());
    if let Some(TileViewerRequest::Pce {
        tile,
        vdc2: request_vdc2,
    }) = state.pending.take()
    {
        vdc2 = request_vdc2 && gfx.vdc2.is_some();
        let id = ui.make_persistent_id("pce_tile_vdc");
        ui.ctx().data_mut(|data| data.insert_persisted(id, vdc2));
        state.select(TileViewerPlatform::Pce, tile % TILE_COUNT);
    }
    let vdc = if vdc2 {
        gfx.vdc2.as_ref().unwrap_or(&gfx.vdc1)
    } else {
        &gfx.vdc1
    };
    let width = COLUMNS * TILE_SIZE;
    let rows = TILE_COUNT / COLUMNS;
    let height = rows * TILE_SIZE;
    let signature = graphics_signature(vdc, &gfx.palette) ^ u64::from(vdc2);
    if state.tracker.last_vram_signature != signature || state.image.size != [width, height] {
        state.tracker.last_vram_signature = signature;
        state.image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        render_tiles(&mut state.image, vdc, &gfx.palette);
    }
    let response = super::common::show_viewer_texture(
        ui,
        &mut state.texture,
        &state.image,
        "pce_tile_viewer",
        "pce_tiles.png",
        2.0,
    );
    if response.clicked()
        && let Some((x, y)) = super::common::hover_pixel_coords(&response, width, height)
    {
        state.select(
            TileViewerPlatform::Pce,
            (y / TILE_SIZE) * COLUMNS + x / TILE_SIZE,
        );
    }
    if state.selected_platform == Some(TileViewerPlatform::Pce)
        && let Some(tile) = state.selected
    {
        super::common::draw_grid_selection(ui, &response, COLUMNS, rows, tile);
        ui.monospace(format!(
            "VDC{} tile ${tile:03X}  VRAM word ${:04X}",
            if vdc2 { 2 } else { 1 },
            tile * 16
        ));
    }
}

fn select_vdc(ui: &mut egui::Ui, id: &str, has_vdc2: bool) -> bool {
    let id = ui.make_persistent_id(id);
    let mut vdc2 = ui
        .ctx()
        .data_mut(|data| data.get_persisted::<bool>(id))
        .unwrap_or(false)
        && has_vdc2;
    if has_vdc2 {
        ui.horizontal(|ui| {
            ui.label("VDC:");
            ui.selectable_value(&mut vdc2, false, "1");
            ui.selectable_value(&mut vdc2, true, "2");
        });
    } else {
        ui.monospace("VDC1");
    }
    ui.ctx().data_mut(|data| data.insert_persisted(id, vdc2));
    vdc2
}

fn render_tiles(
    image: &mut egui::ColorImage,
    vdc: &crate::debug::PceVdcGraphicsData,
    palette: &[zeff_pce_core::hardware::VceColor; 512],
) {
    let mode = color_mode(&vdc.registers);
    for tile in 0..TILE_COUNT {
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let rgba = palette_rgba(palette, tile_pixel(&vdc.vram, tile, x, y, mode));
                image[(
                    (tile % COLUMNS) * TILE_SIZE + x,
                    (tile / COLUMNS) * TILE_SIZE + y,
                )] = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

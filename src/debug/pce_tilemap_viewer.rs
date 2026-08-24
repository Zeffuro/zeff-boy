use crate::debug::pce_graphics::{
    TILE_SIZE, background_dimensions, graphics_signature, map_palette_index, palette_rgba,
};
use crate::debug::{
    DebugTab, DebugUiActions, PceGraphicsData, TileViewerRequest, TileViewerState,
    TilemapViewerState,
};

pub(super) fn draw_pce_tilemap_viewer_content(
    ui: &mut egui::Ui,
    gfx: &PceGraphicsData,
    state: &mut TilemapViewerState,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    let vdc2 = select_vdc(ui, "pce_tilemap_vdc", gfx.vdc2.is_some());
    let vdc = if vdc2 {
        gfx.vdc2.as_ref().unwrap_or(&gfx.vdc1)
    } else {
        &gfx.vdc1
    };
    let (columns, rows) = background_dimensions(&vdc.registers);
    let width = columns * TILE_SIZE;
    let height = rows * TILE_SIZE;
    let signature = graphics_signature(vdc, &gfx.palette) ^ u64::from(vdc2);
    if state.tracker.last_vram_signature != signature || state.image.size != [width, height] {
        state.tracker.last_vram_signature = signature;
        state.image = egui::ColorImage::filled([width, height], egui::Color32::BLACK);
        render_map(&mut state.image, vdc, &gfx.palette);
    }
    let response = super::common::show_viewer_texture(
        ui,
        &mut state.texture,
        &state.image,
        "pce_tilemap_viewer",
        "pce_tilemap.png",
        1.0,
    );
    if let Some((x, y)) = super::common::hover_pixel_coords(&response, width, height) {
        let column = x / TILE_SIZE;
        let row = y / TILE_SIZE;
        let entry_index = row * columns + column;
        let entry = vdc.vram.get(entry_index).copied().unwrap_or(0);
        if response.clicked() {
            state.selected = Some(entry_index);
            tiles.pending = Some(TileViewerRequest::Pce {
                tile: usize::from(entry & 0x0FFF),
                vdc2,
            });
            actions.focus_tab = Some(DebugTab::TileViewer);
        }
        if state.selected == Some(entry_index) {
            super::common::draw_grid_selection(ui, &response, columns, rows, entry_index);
        }
        ui.monospace(format!(
            "VDC{} ({column:03},{row:03}) BAT ${entry_index:04X}  tile ${:03X}  palette {}",
            if vdc2 { 2 } else { 1 },
            entry & 0x0FFF,
            entry >> 12
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

fn render_map(
    image: &mut egui::ColorImage,
    vdc: &crate::debug::PceVdcGraphicsData,
    palette: &[zeff_pce_core::hardware::VceColor; 512],
) {
    let (columns, rows) = background_dimensions(&vdc.registers);
    for y in 0..rows * TILE_SIZE {
        for x in 0..columns * TILE_SIZE {
            let rgba = palette_rgba(palette, map_palette_index(vdc, x, y));
            image[(x, y)] =
                egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
        }
    }
}

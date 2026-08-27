mod gb;
mod gba;
mod nes;
mod sega8;

use crate::debug::common::{color32, debug_colors};
use crate::debug::types::{ConsoleGraphicsData, OamDebugInfo, OamViewerState};
use crate::debug::{DebugTab, DebugUiActions, TileViewerRequest, TileViewerState};

#[derive(Clone, Copy)]
struct AtlasLayout {
    columns: usize,
    cell_width: usize,
    cell_height: usize,
    default_zoom: u8,
}

const GB_NES_ATLAS: AtlasLayout = AtlasLayout {
    columns: 8,
    cell_width: 10,
    cell_height: 18,
    default_zoom: 3,
};

pub(super) fn draw_oam_viewer_content(
    ui: &mut egui::Ui,
    info: &OamDebugInfo,
    graphics: Option<&ConsoleGraphicsData>,
    state: &mut OamViewerState,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    match graphics {
        Some(ConsoleGraphicsData::Gb(gfx)) => gb::draw(ui, info, gfx, state, tiles, actions),
        Some(ConsoleGraphicsData::Gba(gfx)) => gba::draw(ui, info, gfx, state, tiles, actions),
        Some(ConsoleGraphicsData::Nes(gfx)) => nes::draw(ui, info, gfx, state, tiles, actions),
        Some(ConsoleGraphicsData::Coleco(gfx)) => sega8::draw(ui, info, gfx, state, tiles, actions),
        Some(ConsoleGraphicsData::Sega8(gfx)) => sega8::draw(ui, info, gfx, state, tiles, actions),
        _ => draw_oam_table(ui, info),
    }
}

fn show_sprite_atlas(
    ui: &mut egui::Ui,
    state: &mut OamViewerState,
    sprite_count: usize,
    layout: AtlasLayout,
) {
    state.selected = state.selected.min(sprite_count.saturating_sub(1));
    let zoom_id = ui.make_persistent_id("oam_sprite_zoom");
    let mut zoom = ui
        .ctx()
        .data_mut(|data| data.get_persisted::<u8>(zoom_id))
        .unwrap_or(layout.default_zoom)
        .clamp(1, 4);
    ui.horizontal(|ui| {
        ui.weak("Zoom");
        for value in 1..=4 {
            ui.selectable_value(&mut zoom, value, format!("{value}x"));
        }
    });
    ui.ctx()
        .data_mut(|data| data.insert_persisted(zoom_id, zoom));

    let texture = state.texture.get_or_insert_with(|| {
        ui.ctx().load_texture(
            "oam_sprites",
            state.image.clone(),
            egui::TextureOptions::NEAREST,
        )
    });
    texture.set(state.image.clone(), egui::TextureOptions::NEAREST);

    let width = state.image.size[0];
    let height = state.image.size[1];
    let scale = atlas_scale(ui.available_width(), width, zoom);
    let response = ui.add(
        egui::Image::new((
            texture.id(),
            egui::vec2(width as f32 * scale, height as f32 * scale),
        ))
        .sense(egui::Sense::click()),
    );

    if response.clicked()
        && let Some((x, y)) = super::common::hover_pixel_coords(&response, width, height)
    {
        let index = (y / layout.cell_height) * layout.columns + x / layout.cell_width;
        if index < sprite_count {
            state.selected = index;
        }
    }

    let cell_size = egui::vec2(
        layout.cell_width as f32 * scale,
        layout.cell_height as f32 * scale,
    );
    let selected_rect = egui::Rect::from_min_size(
        response.rect.min
            + egui::vec2(
                (state.selected % layout.columns) as f32 * cell_size.x,
                (state.selected / layout.columns) as f32 * cell_size.y,
            ),
        cell_size,
    );
    ui.painter().rect_stroke(
        selected_rect,
        0.0,
        egui::Stroke::new(2.0, color32(debug_colors(ui).selection)),
        egui::StrokeKind::Inside,
    );
}

fn atlas_scale(available_width: f32, image_width: usize, zoom: u8) -> f32 {
    let fit = (available_width / image_width as f32).floor().max(1.0);
    f32::from(zoom.clamp(1, 4)).min(fit)
}

fn resize_atlas(image: &mut egui::ColorImage, sprite_count: usize, layout: AtlasLayout) {
    let rows = sprite_count.div_ceil(layout.columns);
    let size = [
        layout.columns * layout.cell_width,
        rows * layout.cell_height,
    ];
    if image.size != size {
        *image = egui::ColorImage::filled(size, egui::Color32::TRANSPARENT);
    }
}

fn fill_checker(image: &mut egui::ColorImage) {
    for y in 0..image.size[1] {
        for x in 0..image.size[0] {
            image[(x, y)] = if ((x / 2) + (y / 2)) & 1 == 0 {
                egui::Color32::from_gray(28)
            } else {
                egui::Color32::from_gray(40)
            };
        }
    }
}

fn detail(ui: &mut egui::Ui, mono: &egui::FontId, label: &str, value: &str) {
    ui.weak(label);
    ui.label(egui::RichText::new(value).font(mono.clone()));
}

fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

fn draw_oam_table(ui: &mut egui::Ui, info: &OamDebugInfo) {
    egui::Grid::new("oam_grid").striped(true).show(ui, |ui| {
        for header in info.headers {
            ui.strong(*header);
        }
        ui.end_row();

        for row in &info.rows {
            for cell in row {
                ui.monospace(cell);
            }
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::atlas_scale;

    #[test]
    fn atlas_zoom_does_not_expand_to_fill_large_docks() {
        assert_eq!(atlas_scale(1_900.0, 80, 3), 3.0);
        assert_eq!(atlas_scale(120.0, 80, 3), 1.0);
    }
}

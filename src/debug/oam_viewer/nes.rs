use super::*;
use crate::debug::common::{color32, debug_colors, debug_mono_font, nes_palette_rgba};
use crate::debug::types::NesGraphicsData;

const SPRITE_COUNT: usize = 64;

pub(super) fn draw(
    ui: &mut egui::Ui,
    info: &OamDebugInfo,
    gfx: &NesGraphicsData,
    state: &mut OamViewerState,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    render_atlas(&mut state.image, gfx);
    show_sprite_atlas(ui, state, SPRITE_COUNT, GB_NES_ATLAS);
    draw_selected(ui, info, gfx, state.selected, tiles, actions);
}

fn render_atlas(image: &mut egui::ColorImage, gfx: &NesGraphicsData) {
    resize_atlas(image, SPRITE_COUNT, GB_NES_ATLAS);
    fill_checker(image);
    let height = sprite_height(gfx);

    for index in 0..SPRITE_COUNT {
        let base = index * 4;
        let tile = gfx.oam[base + 1];
        let attributes = gfx.oam[base + 2];
        let cell_x = (index % GB_NES_ATLAS.columns) * GB_NES_ATLAS.cell_width + 1;
        let cell_y =
            (index / GB_NES_ATLAS.columns) * GB_NES_ATLAS.cell_height + 1 + (16 - height) / 2;

        for y in 0..height {
            for x in 0..8 {
                let source_x = if attributes & 0x40 != 0 { 7 - x } else { x };
                let source_y = if attributes & 0x80 != 0 {
                    height - 1 - y
                } else {
                    y
                };
                let color_id = super::super::nes_tile_viewer::decode_nes_tile_pixel(
                    &gfx.chr_data,
                    sprite_tile_addr(gfx, tile, source_y),
                    source_y % 8,
                    source_x,
                );
                if color_id == 0 {
                    continue;
                }
                let rgba = nes_palette_rgba(
                    &gfx.palette_ram,
                    4 + (attributes & 0x03),
                    color_id,
                    &gfx.palette_lut,
                );
                image[(cell_x + x, cell_y + y)] =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

fn draw_selected(
    ui: &mut egui::Ui,
    info: &OamDebugInfo,
    gfx: &NesGraphicsData,
    index: usize,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    let base = index * 4;
    let raw = &gfx.oam[base..base + 4];
    let y = raw[0];
    let tile = raw[1];
    let attributes = raw[2];
    let x = raw[3];
    let height = sprite_height(gfx);
    let mono = debug_mono_font(ui);
    let colors = debug_colors(ui);

    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(format!("Sprite {index:02}"))
                .font(mono.clone())
                .color(color32(colors.selection))
                .strong(),
        );
        ui.label(
            egui::RichText::new(format!("OAM +${base:02X}"))
                .font(mono.clone())
                .color(color32(colors.address)),
        );
        ui.label(
            egui::RichText::new(format!(
                "raw {}",
                raw.iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ))
            .font(mono.clone())
            .color(color32(colors.opcode)),
        );
        if ui.small_button("Open Tile").clicked() {
            let pattern = sprite_tile_addr(gfx, tile, 0);
            tiles.pending = Some(TileViewerRequest::Nes {
                tile: (pattern % 0x1000) / 16,
                table: pattern / 0x1000,
                palette: attributes & 3,
            });
            actions.focus_tab = Some(DebugTab::TileViewer);
        }
    });

    egui::Grid::new("selected_nes_oam_details")
        .num_columns(4)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            detail(
                ui,
                &mono,
                "Position",
                &format!("{x}, {}", y.wrapping_add(1)),
            );
            detail(ui, &mono, "Size", &format!("8x{height}"));
            ui.end_row();
            detail(ui, &mono, "Tile", &format!("${tile:02X}"));
            detail(ui, &mono, "Palette", &format!("OBJ {}", attributes & 3));
            ui.end_row();
            detail(
                ui,
                &mono,
                "Flip",
                &format!(
                    "X:{} Y:{}",
                    yes_no(attributes & 0x40 != 0),
                    yes_no(attributes & 0x80 != 0)
                ),
            );
            detail(
                ui,
                &mono,
                "Priority",
                if attributes & 0x20 != 0 {
                    "Behind BG"
                } else {
                    "Above BG"
                },
            );
            ui.end_row();
            detail(
                ui,
                &mono,
                "Pattern",
                &format!("${:04X}", sprite_tile_addr(gfx, tile, 0)),
            );
            ui.end_row();
        });

    if let Some(row) = info.rows.get(index) {
        ui.collapsing("Raw fields", |ui| {
            egui::Grid::new("selected_nes_oam_raw_fields")
                .striped(true)
                .show(ui, |ui| {
                    for (header, value) in info.headers.iter().zip(row) {
                        ui.weak(*header);
                        ui.monospace(value);
                        ui.end_row();
                    }
                });
        });
    }
}

fn sprite_height(gfx: &NesGraphicsData) -> usize {
    if gfx.ctrl & 0x20 != 0 { 16 } else { 8 }
}

fn sprite_tile_addr(gfx: &NesGraphicsData, tile: u8, source_y: usize) -> usize {
    if sprite_height(gfx) == 16 {
        let table = (tile as usize & 1) * 0x1000;
        table + ((tile as usize & 0xFE) + source_y / 8) * 16
    } else {
        let table = if gfx.ctrl & 0x08 != 0 { 0x1000 } else { 0 };
        table + tile as usize * 16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graphics(ctrl: u8) -> NesGraphicsData {
        let mut chr_data = vec![0; 0x2000];
        chr_data[0] = 0x80;
        let mut palette_ram = [0; 32];
        palette_ram[17] = 1;
        let mut palette_lut = [[0; 4]; 64];
        palette_lut[1] = [255, 80, 40, 255];
        NesGraphicsData {
            chr_data,
            nametable_data: vec![0; 0x1000],
            oam: [0; 256],
            palette_ram,
            palette_lut,
            ctrl,
            mirroring: zeff_nes_core::hardware::cartridge::Mirroring::Horizontal,
            scroll_t: 0,
            fine_x: 0,
        }
    }

    #[test]
    fn renders_sprite_pixels() {
        let gfx = graphics(0);
        let mut image = egui::ColorImage::filled([80, 90], egui::Color32::TRANSPARENT);
        render_atlas(&mut image, &gfx);

        assert_eq!(image.size, [80, 144]);
        assert_eq!(image[(1, 5)], egui::Color32::from_rgb(255, 80, 40));
    }

    #[test]
    fn tall_sprites_select_table_and_tile_pair() {
        let gfx = graphics(0x20);

        assert_eq!(sprite_tile_addr(&gfx, 0x03, 0), 0x1020);
        assert_eq!(sprite_tile_addr(&gfx, 0x03, 8), 0x1030);
    }
}

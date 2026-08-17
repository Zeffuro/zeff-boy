use super::*;
use crate::debug::common::{color32, debug_colors, debug_mono_font, sega8_palette_rgba};
use crate::debug::types::Sega8GraphicsData;
use zeff_sega8_core::hardware::constants::{
    MODE4_SPRITE_TERMINATOR_Y, MODE4_SPRITE_X_TILE_TABLE_OFFSET,
};

const SPRITE_COUNT: usize = 64;
const SEGA_ATLAS: AtlasLayout = AtlasLayout {
    columns: 8,
    cell_width: 18,
    cell_height: 34,
    default_zoom: 2,
};

pub(super) fn draw(
    ui: &mut egui::Ui,
    info: &OamDebugInfo,
    gfx: &Sega8GraphicsData,
    state: &mut OamViewerState,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    if !gfx.mode4.enabled {
        draw_oam_table(ui, info);
        ui.weak("Visual sprites currently require Mode 4.");
        return;
    }
    render_atlas(&mut state.image, gfx);
    show_sprite_atlas(ui, state, SPRITE_COUNT, SEGA_ATLAS);
    draw_selected(ui, info, gfx, state.selected, tiles, actions);
}

fn render_atlas(image: &mut egui::ColorImage, gfx: &Sega8GraphicsData) {
    resize_atlas(image, SPRITE_COUNT, SEGA_ATLAS);
    fill_checker(image);
    let scale = if gfx.mode4.sprite_magnified { 2 } else { 1 };
    let width = gfx.mode4.sprite_width;
    let height = gfx.mode4.sprite_height;
    let base_height = height / scale;
    let mut terminated = false;

    for index in 0..SPRITE_COUNT {
        let sprite = sprite(gfx, index, base_height);
        if sprite.y_raw == MODE4_SPRITE_TERMINATOR_Y {
            terminated = true;
        }
        if terminated {
            continue;
        }
        let cell_x = (index % SEGA_ATLAS.columns) * SEGA_ATLAS.cell_width;
        let cell_y = (index / SEGA_ATLAS.columns) * SEGA_ATLAS.cell_height;
        let offset_x = cell_x + (SEGA_ATLAS.cell_width - width) / 2;
        let offset_y = cell_y + (SEGA_ATLAS.cell_height - height) / 2;
        for y in 0..height {
            for x in 0..width {
                let source_x = x / scale;
                let source_y = y / scale;
                let tile = sprite.tile + source_y / 8;
                let color = crate::debug::sega8_tile_viewer::pattern_color(
                    &gfx.vram,
                    gfx.mode4.sprite_pattern_base / 32 + tile,
                    source_x,
                    source_y % 8,
                );
                if color == 0 {
                    continue;
                }
                let rgba = sega8_palette_rgba(gfx.system, &gfx.cram, 16 + color);
                image[(offset_x + x, offset_y + y)] =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

fn draw_selected(
    ui: &mut egui::Ui,
    info: &OamDebugInfo,
    gfx: &Sega8GraphicsData,
    index: usize,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    let scale = if gfx.mode4.sprite_magnified { 2 } else { 1 };
    let base_height = gfx.mode4.sprite_height / scale;
    let sprite = sprite(gfx, index, base_height);
    let y_addr = gfx.sprite_table_base + index;
    let xt_addr = gfx.sprite_table_base + MODE4_SPRITE_X_TILE_TABLE_OFFSET + index * 2;
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
            egui::RichText::new(format!("Y ${y_addr:04X}  XT ${xt_addr:04X}"))
                .font(mono.clone())
                .color(color32(colors.address)),
        );
        if ui.small_button("Open Tile").clicked() {
            tiles.pending = Some(TileViewerRequest::Sega8 {
                tile: gfx.mode4.sprite_pattern_base / 32 + sprite.tile,
            });
            actions.focus_tab = Some(DebugTab::TileViewer);
        }
    });

    egui::Grid::new("selected_sega8_oam_details")
        .num_columns(4)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            detail(
                ui,
                &mono,
                "Position",
                &format!("{}, {}", sprite.x, sprite.y),
            );
            detail(
                ui,
                &mono,
                "Size",
                &format!("{}x{}", gfx.mode4.sprite_width, gfx.mode4.sprite_height),
            );
            ui.end_row();
            detail(ui, &mono, "Tile", &format!("${:02X}", sprite.tile));
            detail(
                ui,
                &mono,
                "Pattern",
                &format!("${:04X}", gfx.mode4.sprite_pattern_base + sprite.tile * 32),
            );
            ui.end_row();
            detail(ui, &mono, "Magnified", yes_no(gfx.mode4.sprite_magnified));
            detail(ui, &mono, "Shift left", yes_no(gfx.mode4.sprite_shift_left));
            ui.end_row();
            detail(
                ui,
                &mono,
                "State",
                if sprite.y_raw == MODE4_SPRITE_TERMINATOR_Y {
                    "Terminator"
                } else {
                    "Active"
                },
            );
            ui.end_row();
        });

    if let Some(row) = info
        .rows
        .iter()
        .find(|row| row.first().and_then(|value| value.parse::<usize>().ok()) == Some(index))
    {
        ui.collapsing("Raw fields", |ui| {
            egui::Grid::new("selected_sega8_oam_raw_fields")
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

#[derive(Clone, Copy)]
struct SegaSprite {
    x: isize,
    y: u8,
    y_raw: u8,
    tile: usize,
}

fn sprite(gfx: &Sega8GraphicsData, index: usize, base_height: usize) -> SegaSprite {
    let y_raw = gfx.oam.get(index).copied().unwrap_or(0);
    let xt = MODE4_SPRITE_X_TILE_TABLE_OFFSET + index * 2;
    let x_shift = if gfx.mode4.sprite_shift_left { -8 } else { 0 };
    let tile = sprite_tile(gfx.oam.get(xt + 1).copied().unwrap_or(0), base_height);
    SegaSprite {
        x: isize::from(gfx.oam.get(xt).copied().unwrap_or(0)) + x_shift,
        y: y_raw.wrapping_add(1),
        y_raw,
        tile,
    }
}

fn sprite_tile(raw: u8, base_height: usize) -> usize {
    usize::from(if base_height == 16 { raw & !1 } else { raw })
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_sega8_core::hardware::cartridge::Sega8System;

    #[test]
    fn tall_sprite_uses_an_even_tile_pair() {
        assert_eq!(sprite_tile(7, 16), 6);
        assert_eq!(sprite_tile(7, 8), 7);
    }

    #[test]
    fn renders_mode4_sprite_pixels() {
        let mut vram = vec![0; 0x4000];
        vram[0] = 0x80;
        let mut cram = vec![0; 0x40];
        cram[17] = 3;
        let mut oam = vec![0; 0x100];
        oam[1] = MODE4_SPRITE_TERMINATOR_Y;
        let gfx = Sega8GraphicsData {
            system: Sega8System::MasterSystem,
            vram,
            cram,
            oam,
            registers: [0; 16],
            status: 0,
            address: 0,
            code: 0,
            v_counter: 0,
            h_counter: 0,
            scanline: 0,
            scanline_cycle: 0,
            line_counter: 0,
            frame_interrupt_enabled: false,
            line_interrupt_enabled: false,
            interrupt_pending: false,
            line_interrupt_pending: false,
            display_enabled: true,
            tms9918_mode: String::new(),
            sprite_table_base: 0,
            mode4: zeff_sega8_core::hardware::vdp::Mode4VdpDebugSnapshot {
                enabled: true,
                name_table_base: 0,
                sprite_table_base: 0,
                sprite_pattern_base: 0,
                horizontal_scroll: 0,
                vertical_scroll: 0,
                backdrop_color_index: 0,
                sprite_height: 8,
                sprite_width: 8,
                max_sprites_per_line: 8,
                horizontal_scroll_lock: false,
                vertical_scroll_lock: false,
                hide_left_column: false,
                sprite_shift_left: false,
                sprite_magnified: false,
            },
        };
        let mut image = egui::ColorImage::filled([1, 1], egui::Color32::TRANSPARENT);

        render_atlas(&mut image, &gfx);

        assert_eq!(image.size, [144, 272]);
        assert_eq!(image[(5, 13)], egui::Color32::from_rgb(255, 0, 0));
    }
}

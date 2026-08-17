use super::*;
use crate::debug::common::{color32, debug_colors, debug_mono_font};
use crate::debug::types::GbGraphicsData;
use zeff_gb_core::hardware::ppu::{
    SpriteEntry, apply_dmg_palette, cgb_palette_rgba, correct_color, decode_tile_pixel,
};

const SPRITE_COUNT: usize = 40;

pub(super) fn draw(
    ui: &mut egui::Ui,
    info: &OamDebugInfo,
    gfx: &GbGraphicsData,
    state: &mut OamViewerState,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    render_atlas(&mut state.image, gfx);
    show_sprite_atlas(ui, state, SPRITE_COUNT, GB_NES_ATLAS);
    draw_selected(ui, info, gfx, state.selected, tiles, actions);
}

fn draw_selected(
    ui: &mut egui::Ui,
    info: &OamDebugInfo,
    gfx: &GbGraphicsData,
    index: usize,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    let sprite = SpriteEntry::from_oam(&gfx.oam, index);
    let height = sprite_height(gfx);
    let base = index * 4;
    let raw = gfx.oam.get(base..base + 4).unwrap_or(&[]);
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
            egui::RichText::new(format!("OAM ${:04X}", 0xFE00 + base))
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
            tiles.pending = Some(TileViewerRequest::Gb {
                tile: if height == 16 {
                    usize::from(sprite.tile & 0xFE)
                } else {
                    usize::from(sprite.tile)
                },
                bank: if gfx.cgb_mode {
                    sprite.cgb_vram_bank()
                } else {
                    0
                },
                obj_palette: true,
                palette: if gfx.cgb_mode {
                    sprite.cgb_obj_palette_index()
                } else {
                    sprite.palette_number()
                },
            });
            actions.focus_tab = Some(DebugTab::TileViewer);
        }
        if ui.small_button("Open OAM").clicked() {
            actions.memory_target = Some((0xFE00 + base) as u32);
            actions.focus_tab = Some(DebugTab::MemoryViewer);
        }
    });

    egui::Grid::new("selected_oam_details")
        .num_columns(4)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            detail(
                ui,
                &mono,
                "Position",
                &format!("{}, {}", sprite.x, sprite.y),
            );
            detail(ui, &mono, "Size", &format!("8x{height}"));
            ui.end_row();
            detail(ui, &mono, "Tile", &format!("${:02X}", sprite.tile));
            detail(
                ui,
                &mono,
                "Palette",
                &if gfx.cgb_mode {
                    format!("CGB {}", sprite.cgb_obj_palette_index())
                } else {
                    format!("OBP{}", sprite.palette_number())
                },
            );
            ui.end_row();
            detail(
                ui,
                &mono,
                "Flip",
                &format!(
                    "X:{} Y:{}",
                    yes_no(sprite.flip_x()),
                    yes_no(sprite.flip_y())
                ),
            );
            detail(
                ui,
                &mono,
                "Priority",
                if sprite.bg_priority() {
                    "Behind BG"
                } else {
                    "Above BG"
                },
            );
            ui.end_row();
            if gfx.cgb_mode {
                detail(ui, &mono, "VRAM bank", &sprite.cgb_vram_bank().to_string());
                ui.end_row();
            }
        });

    if let Some(row) = info.rows.get(index) {
        ui.collapsing("Raw fields", |ui| {
            egui::Grid::new("selected_oam_raw_fields")
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

fn render_atlas(image: &mut egui::ColorImage, gfx: &GbGraphicsData) {
    resize_atlas(image, SPRITE_COUNT, GB_NES_ATLAS);
    fill_checker(image);
    let height = sprite_height(gfx);

    for index in 0..SPRITE_COUNT {
        let sprite = SpriteEntry::from_oam(&gfx.oam, index);
        let cell_x = (index % GB_NES_ATLAS.columns) * GB_NES_ATLAS.cell_width + 1;
        let cell_y =
            (index / GB_NES_ATLAS.columns) * GB_NES_ATLAS.cell_height + 1 + (16 - height) / 2;
        for y in 0..height {
            for x in 0..8 {
                let source_x = if sprite.flip_x() { 7 - x } else { x };
                let source_y = if sprite.flip_y() { height - 1 - y } else { y };
                let tile = if height == 16 {
                    (sprite.tile & 0xFE).wrapping_add((source_y / 8) as u8)
                } else {
                    sprite.tile
                };
                let bank_base = if gfx.cgb_mode {
                    sprite.cgb_vram_bank() * 0x2000
                } else {
                    0
                };
                let color_id = decode_tile_pixel(
                    &gfx.vram,
                    bank_base + tile as usize * 16,
                    source_y % 8,
                    source_x,
                );
                if color_id == 0 {
                    continue;
                }
                let rgba = if gfx.cgb_mode {
                    correct_color(
                        cgb_palette_rgba(
                            &gfx.obj_palette_ram,
                            sprite.cgb_obj_palette_index(),
                            color_id,
                        ),
                        gfx.color_correction,
                        gfx.color_correction_matrix,
                    )
                } else {
                    let palette = if sprite.palette_number() == 0 {
                        gfx.ppu.obp0
                    } else {
                        gfx.ppu.obp1
                    };
                    apply_dmg_palette(gfx.dmg_palette_preset, palette, color_id)
                };
                image[(cell_x + x, cell_y + y)] =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

fn sprite_height(gfx: &GbGraphicsData) -> usize {
    if gfx.ppu.lcdc & 0x04 != 0 { 16 } else { 8 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ColorCorrection, DmgPalettePreset};

    #[test]
    fn renders_non_transparent_sprite_pixels() {
        let mut vram = vec![0; 0x4000];
        vram[0] = 0x80;
        let gfx = GbGraphicsData {
            vram,
            oam: vec![0; 160],
            ppu: zeff_gb_core::debug::PpuSnapshot {
                lcdc: 0,
                stat: 0,
                scy: 0,
                scx: 0,
                ly: 0,
                lyc: 0,
                wy: 0,
                wx: 0,
                bgp: 0xE4,
                obp0: 0xE4,
                obp1: 0xE4,
                cycles: 0,
                window_line_counter: 0,
                window_y_triggered: false,
                window_was_active_this_frame: false,
                window_visible_on_current_line: false,
                rendered_current_line: false,
                draw_dots_for_line: 0,
            },
            cgb_mode: false,
            bg_palette_ram: [0; 64],
            obj_palette_ram: [0; 64],
            color_correction: ColorCorrection::None,
            color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            dmg_palette_preset: DmgPalettePreset::default(),
        };
        let mut image = egui::ColorImage::filled([80, 90], egui::Color32::TRANSPARENT);
        render_atlas(&mut image, &gfx);

        assert_ne!(image[(1, 5)], egui::Color32::from_gray(28));
    }
}

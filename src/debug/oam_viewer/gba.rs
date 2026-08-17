use super::*;
use crate::debug::common::{color32, debug_colors, debug_mono_font};
use crate::debug::types::GbaGraphicsData;

const SPRITE_COUNT: usize = 128;
const GBA_ATLAS: AtlasLayout = AtlasLayout {
    columns: 8,
    cell_width: 18,
    cell_height: 18,
    default_zoom: 2,
};

pub(super) fn draw(
    ui: &mut egui::Ui,
    info: &OamDebugInfo,
    gfx: &GbaGraphicsData,
    state: &mut OamViewerState,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    render_atlas(&mut state.image, gfx);
    show_sprite_atlas(ui, state, SPRITE_COUNT, GBA_ATLAS);
    draw_selected(ui, info, gfx, state.selected, tiles, actions);
}

fn render_atlas(image: &mut egui::ColorImage, gfx: &GbaGraphicsData) {
    resize_atlas(image, SPRITE_COUNT, GBA_ATLAS);
    fill_checker(image);

    for index in 0..SPRITE_COUNT {
        let sprite = GbaSprite::read(&gfx.oam, index);
        if sprite.disabled || sprite.dimensions.is_none() {
            continue;
        }
        let (width, height) = sprite.dimensions.unwrap_or((8, 8));
        let draw_width = if sprite.double_size { width * 2 } else { width };
        let draw_height = if sprite.double_size {
            height * 2
        } else {
            height
        };
        let scale = (16.0 / draw_width.max(draw_height) as f32).min(1.0);
        let thumb_width = (draw_width as f32 * scale).round().max(1.0) as usize;
        let thumb_height = (draw_height as f32 * scale).round().max(1.0) as usize;
        let cell_x = (index % GBA_ATLAS.columns) * GBA_ATLAS.cell_width;
        let cell_y = (index / GBA_ATLAS.columns) * GBA_ATLAS.cell_height;
        let offset_x = cell_x + (GBA_ATLAS.cell_width - thumb_width) / 2;
        let offset_y = cell_y + (GBA_ATLAS.cell_height - thumb_height) / 2;
        let affine = sprite
            .affine
            .then(|| affine_params(&gfx.oam, sprite.affine_index));

        for y in 0..thumb_height {
            for x in 0..thumb_width {
                let draw_x = x * draw_width / thumb_width;
                let draw_y = y * draw_height / thumb_height;
                let Some((source_x, source_y)) = source_pixel(
                    (draw_x, draw_y),
                    (width, height),
                    (draw_width, draw_height),
                    (sprite.hflip, sprite.vflip),
                    affine,
                ) else {
                    continue;
                };
                let color_index = obj_color_index(gfx, &sprite, width, source_x, source_y);
                if color_index == 0 {
                    continue;
                }
                let palette_index = if sprite.color_256 {
                    0x100 + color_index
                } else {
                    0x100 + sprite.palette * 16 + color_index
                };
                let rgba = crate::debug::gba_tile_viewer::gba_palette_rgba(
                    &gfx.palette_ram,
                    palette_index,
                );
                image[(offset_x + x, offset_y + y)] =
                    egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
    }
}

fn draw_selected(
    ui: &mut egui::Ui,
    info: &OamDebugInfo,
    gfx: &GbaGraphicsData,
    index: usize,
    tiles: &mut TileViewerState,
    actions: &mut DebugUiActions,
) {
    let sprite = GbaSprite::read(&gfx.oam, index);
    let base = index * 8;
    let raw = gfx.oam.get(base..base + 8).unwrap_or(&[]);
    let mono = debug_mono_font(ui);
    let colors = debug_colors(ui);

    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(format!("Sprite {index:03}"))
                .font(mono.clone())
                .color(color32(colors.selection))
                .strong(),
        );
        ui.label(
            egui::RichText::new(format!("OAM ${:08X}", 0x0700_0000 + base))
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
            tiles.pending = Some(TileViewerRequest::Gba {
                tile: sprite.effective_tile(gfx.ppu.obj_mapping_1d),
                color_256: sprite.color_256,
                palette: sprite.palette,
            });
            actions.focus_tab = Some(DebugTab::TileViewer);
        }
        if ui.small_button("Open OAM").clicked() {
            actions.memory_target = Some((0x0700_0000 + base) as u32);
            actions.focus_tab = Some(DebugTab::MemoryViewer);
        }
    });

    let (width, height) = sprite.dimensions.unwrap_or((0, 0));
    let palette_label = if sprite.color_256 {
        "OBJ 256".to_owned()
    } else {
        format!("OBJ {}", sprite.palette)
    };
    egui::Grid::new("selected_gba_oam_details")
        .num_columns(4)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            detail(
                ui,
                &mono,
                "Position",
                &format!("{}, {}", sprite.x, sprite.y),
            );
            detail(ui, &mono, "Size", &format!("{width}x{height}"));
            ui.end_row();
            detail(ui, &mono, "Tile", &format!("${:03X}", sprite.tile));
            detail(ui, &mono, "Palette", &palette_label);
            ui.end_row();
            detail(
                ui,
                &mono,
                "Format",
                if sprite.color_256 { "8bpp" } else { "4bpp" },
            );
            detail(ui, &mono, "Priority", &sprite.priority.to_string());
            ui.end_row();
            detail(ui, &mono, "Mode", sprite.mode_label());
            detail(
                ui,
                &mono,
                "Mapping",
                if gfx.ppu.obj_mapping_1d { "1D" } else { "2D" },
            );
            ui.end_row();
            if sprite.affine {
                detail(
                    ui,
                    &mono,
                    "Affine",
                    &format!("matrix {}", sprite.affine_index),
                );
                detail(ui, &mono, "Double", yes_no(sprite.double_size));
            } else {
                detail(
                    ui,
                    &mono,
                    "Flip",
                    &format!("X:{} Y:{}", yes_no(sprite.hflip), yes_no(sprite.vflip)),
                );
                detail(ui, &mono, "Disabled", yes_no(sprite.disabled));
            }
            ui.end_row();
        });

    if let Some(row) = info
        .rows
        .iter()
        .find(|row| row.first().and_then(|value| value.parse::<usize>().ok()) == Some(index))
    {
        ui.collapsing("Raw fields", |ui| {
            egui::Grid::new("selected_gba_oam_raw_fields")
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
struct GbaSprite {
    attr0: u16,
    tile: usize,
    palette: usize,
    priority: u8,
    x: i32,
    y: i32,
    dimensions: Option<(usize, usize)>,
    affine: bool,
    affine_index: usize,
    double_size: bool,
    disabled: bool,
    hflip: bool,
    vflip: bool,
    color_256: bool,
}

impl GbaSprite {
    fn read(oam: &[u8], index: usize) -> Self {
        let base = index * 8;
        let attr0 = read_u16(oam, base);
        let attr1 = read_u16(oam, base + 2);
        let attr2 = read_u16(oam, base + 4);
        let affine = attr0 & 0x0100 != 0;
        Self {
            attr0,
            tile: usize::from(attr2 & 0x03FF),
            palette: usize::from((attr2 >> 12) & 0xF),
            priority: ((attr2 >> 10) & 3) as u8,
            x: sign_coord(attr1 & 0x01FF, 512),
            y: y_coord(attr0 & 0x00FF),
            dimensions: dimensions((attr0 >> 14) & 3, (attr1 >> 14) & 3),
            affine,
            affine_index: usize::from((attr1 >> 9) & 0x1F),
            double_size: affine && attr0 & 0x0200 != 0,
            disabled: !affine && attr0 & 0x0200 != 0,
            hflip: !affine && attr1 & 0x1000 != 0,
            vflip: !affine && attr1 & 0x2000 != 0,
            color_256: attr0 & 0x2000 != 0,
        }
    }

    fn effective_tile(self, one_dimensional: bool) -> usize {
        if self.color_256 && !one_dimensional {
            self.tile & !1
        } else {
            self.tile
        }
    }

    fn mode_label(self) -> &'static str {
        match (self.attr0 >> 10) & 3 {
            0 => "Normal",
            1 => "Alpha",
            2 => "OBJ window",
            _ => "Prohibited",
        }
    }
}

fn obj_color_index(
    gfx: &GbaGraphicsData,
    sprite: &GbaSprite,
    width: usize,
    x: usize,
    y: usize,
) -> usize {
    let stride = if sprite.color_256 { 2 } else { 1 };
    let tile_base = sprite.effective_tile(gfx.ppu.obj_mapping_1d);
    let tile_x = x / 8;
    let tile_y = y / 8;
    let tile = if gfx.ppu.obj_mapping_1d {
        tile_base + (tile_y * (width / 8) + tile_x) * stride
    } else {
        tile_base + tile_y * 32 + tile_x * stride
    };
    if gfx.ppu.display_mode >= 3 && tile < 512 {
        return 0;
    }
    let base = 0x1_0000 + tile * 32;
    let x = x & 7;
    let y = y & 7;
    if sprite.color_256 {
        usize::from(gfx.vram.get(base + y * 8 + x).copied().unwrap_or(0))
    } else {
        let byte = gfx.vram.get(base + y * 4 + x / 2).copied().unwrap_or(0);
        usize::from(if x & 1 == 0 { byte & 0x0F } else { byte >> 4 })
    }
}

fn source_pixel(
    position: (usize, usize),
    source_size: (usize, usize),
    draw_size: (usize, usize),
    flips: (bool, bool),
    affine: Option<(i32, i32, i32, i32)>,
) -> Option<(usize, usize)> {
    let (x, y) = position;
    let (width, height) = source_size;
    let (draw_width, draw_height) = draw_size;
    let (hflip, vflip) = flips;
    if let Some((pa, pb, pc, pd)) = affine {
        let rel_x = x as i32 - draw_width as i32 / 2;
        let rel_y = y as i32 - draw_height as i32 / 2;
        let source_x = ((pa * rel_x + pb * rel_y) >> 8) + width as i32 / 2;
        let source_y = ((pc * rel_x + pd * rel_y) >> 8) + height as i32 / 2;
        if !(0..width as i32).contains(&source_x) || !(0..height as i32).contains(&source_y) {
            None
        } else {
            Some((source_x as usize, source_y as usize))
        }
    } else {
        Some((
            if hflip { width - 1 - x } else { x },
            if vflip { height - 1 - y } else { y },
        ))
    }
}

fn affine_params(oam: &[u8], index: usize) -> (i32, i32, i32, i32) {
    let base = index * 0x20;
    (
        i32::from(read_i16(oam, base + 0x06)),
        i32::from(read_i16(oam, base + 0x0E)),
        i32::from(read_i16(oam, base + 0x16)),
        i32::from(read_i16(oam, base + 0x1E)),
    )
}

fn dimensions(shape: u16, size: u16) -> Option<(usize, usize)> {
    match (shape, size) {
        (0, 0) => Some((8, 8)),
        (0, 1) => Some((16, 16)),
        (0, 2) => Some((32, 32)),
        (0, 3) => Some((64, 64)),
        (1, 0) => Some((16, 8)),
        (1, 1) => Some((32, 8)),
        (1, 2) => Some((32, 16)),
        (1, 3) => Some((64, 32)),
        (2, 0) => Some((8, 16)),
        (2, 1) => Some((8, 32)),
        (2, 2) => Some((16, 32)),
        (2, 3) => Some((32, 64)),
        _ => None,
    }
}

fn sign_coord(value: u16, range: i32) -> i32 {
    let value = i32::from(value);
    if value >= range / 2 {
        value - range
    } else {
        value
    }
}

fn y_coord(value: u16) -> i32 {
    let value = i32::from(value & 0x00FF);
    if value >= 160 { value - 256 } else { value }
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}

fn read_i16(data: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_all_valid_dimensions() {
        assert_eq!(dimensions(0, 3), Some((64, 64)));
        assert_eq!(dimensions(1, 2), Some((32, 16)));
        assert_eq!(dimensions(2, 1), Some((8, 32)));
        assert_eq!(dimensions(3, 0), None);
    }

    #[test]
    fn renders_obj_palette_pixels() {
        let mut vram = vec![0; 0x1_8000];
        vram[0x1_0000] = 1;
        let mut palette_ram = vec![0; 0x400];
        palette_ram[0x202] = 0x1F;
        let gfx = GbaGraphicsData {
            vram,
            palette_ram,
            oam: vec![0; 0x400],
            ppu: zeff_gba_core::hardware::ppu::PpuDebugSnapshot {
                dispcnt: 0,
                bgcnt: [0; 4],
                vcount: 0,
                in_vblank: false,
                display_mode: 0,
                bg_enabled: [false; 4],
                obj_enabled: true,
                obj_mapping_1d: true,
                debug_flags: Default::default(),
                non_black_pixels: 0,
            },
        };
        let mut image = egui::ColorImage::filled([1, 1], egui::Color32::TRANSPARENT);

        render_atlas(&mut image, &gfx);

        assert_eq!(image.size, [144, 288]);
        assert_eq!(image[(5, 5)], egui::Color32::from_rgb(255, 0, 0));
    }
}

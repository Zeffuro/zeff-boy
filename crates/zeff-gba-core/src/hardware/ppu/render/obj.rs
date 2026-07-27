use super::effects::{ColorEffects, Layer, Mosaic};
use super::window::Windows;
use super::{
    blend_obj_under_current_top, blend_obj_under_current_top_line, draw_color, draw_color_line,
    read_i16, read_le16,
};
use crate::hardware::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::hardware::ppu::Ppu;

impl Ppu {
    pub(super) fn render_objs(
        &mut self,
        dispcnt: u16,
        palette_ram: &[u8],
        vram: &[u8],
        oam: &[u8],
        bg_priorities: &[u8],
        bg_second_priorities: &[u8],
        obj_priorities: &mut [u8],
        pixel_layers: &mut [Layer],
        pixel_colors: &mut [u16],
        effects: ColorEffects,
        windows: &Windows,
        mosaic: Mosaic,
    ) {
        let one_dimensional = dispcnt & (1 << 6) != 0;
        for obj in (0..128usize).rev() {
            let base = obj * 8;
            let attr0 = read_le16(oam, base);
            let attr1 = read_le16(oam, base + 2);
            let attr2 = read_le16(oam, base + 4);
            let affine = attr0 & (1 << 8) != 0;
            if !affine && attr0 & (1 << 9) != 0 {
                continue;
            }
            if attr0 & 0x0C00 >= 0x0800 {
                continue;
            }
            let semi_transparent = attr0 & 0x0C00 == 0x0400;
            let color_256 = attr0 & (1 << 13) != 0;
            let shape = (attr0 >> 14) & 0x3;
            let size = (attr1 >> 14) & 0x3;
            let Some((width, height)) = obj_dimensions(shape, size) else {
                continue;
            };
            let double_size = affine && attr0 & (1 << 9) != 0;
            let draw_width = if double_size { width * 2 } else { width };
            let draw_height = if double_size { height * 2 } else { height };
            let y = obj_y_coord(attr0 & 0x00FF);
            let x = sign_obj_coord(attr1 & 0x01FF, 512);
            let hflip = !affine && attr1 & (1 << 12) != 0;
            let vflip = !affine && attr1 & (1 << 13) != 0;
            let tile_base = usize::from(attr2 & 0x03FF);
            let obj_priority = ((attr2 >> 10) & 0x3) as u8;
            let palette_bank = usize::from((attr2 >> 12) & 0xF);
            let affine_params = affine.then(|| obj_affine_params(oam, (attr1 >> 9) & 0x1F));
            let use_mosaic = attr0 & (1 << 12) != 0;
            for py in 0..draw_height {
                let screen_y = y + py as i32;
                if !(0..SCREEN_HEIGHT as i32).contains(&screen_y) {
                    continue;
                }
                for px in 0..draw_width {
                    let screen_x = x + px as i32;
                    if !(0..SCREEN_WIDTH as i32).contains(&screen_x) {
                        continue;
                    }
                    if !windows.allows_obj(screen_x as usize, screen_y as usize) {
                        continue;
                    }
                    let dst = screen_y as usize * SCREEN_WIDTH + screen_x as usize;
                    let (sample_px, sample_py) = mosaic.obj_sample(px, py, use_mosaic);
                    let Some((src_x, src_y)) = obj_source_pixel(
                        sample_px,
                        sample_py,
                        width,
                        height,
                        draw_width,
                        draw_height,
                        hflip,
                        vflip,
                        affine_params,
                    ) else {
                        continue;
                    };
                    let color_index = obj_color_index(ObjColorParams {
                        vram,
                        tile_base,
                        x: src_x,
                        y: src_y,
                        width,
                        color_256,
                        one_dimensional,
                        bitmap_obj_tiles: dispcnt & 0x7 >= 3,
                    });
                    if color_index == 0 {
                        continue;
                    }
                    let palette_index = if color_256 {
                        0x100 + usize::from(color_index)
                    } else {
                        0x100 + palette_bank * 16 + usize::from(color_index)
                    };
                    let color = read_le16(palette_ram, palette_index * 2);
                    if obj_priority > obj_priorities[dst] {
                        continue;
                    }
                    if obj_priority > bg_priorities[dst] {
                        if obj_priority <= bg_second_priorities[dst] {
                            blend_obj_under_current_top(
                                &mut self.framebuffer,
                                pixel_layers,
                                pixel_colors,
                                screen_x as usize,
                                screen_y as usize,
                                color,
                                effects,
                                windows.allows_effect(screen_x as usize, screen_y as usize),
                            );
                        }
                        continue;
                    }
                    obj_priorities[dst] = obj_priority;
                    draw_color(
                        &mut self.framebuffer,
                        pixel_layers,
                        pixel_colors,
                        screen_x as usize,
                        screen_y as usize,
                        color,
                        Layer::Obj,
                        effects,
                        semi_transparent,
                        windows.allows_effect(screen_x as usize, screen_y as usize),
                    );
                }
            }
        }
    }

    pub(super) fn render_objs_line(
        &mut self,
        dispcnt: u16,
        palette_ram: &[u8],
        vram: &[u8],
        oam: &[u8],
        y: usize,
        bg_priorities: &[u8; SCREEN_WIDTH],
        bg_second_priorities: &[u8; SCREEN_WIDTH],
        obj_priorities: &mut [u8; SCREEN_WIDTH],
        pixel_layers: &mut [Layer; SCREEN_WIDTH],
        pixel_colors: &mut [u16; SCREEN_WIDTH],
        effects: ColorEffects,
        windows: &Windows,
        mosaic: Mosaic,
    ) {
        let one_dimensional = dispcnt & (1 << 6) != 0;
        let screen_y = y as i32;
        for obj in (0..128usize).rev() {
            let base = obj * 8;
            let attr0 = read_le16(oam, base);
            let attr1 = read_le16(oam, base + 2);
            let attr2 = read_le16(oam, base + 4);
            let affine = attr0 & (1 << 8) != 0;
            if !affine && attr0 & (1 << 9) != 0 {
                continue;
            }
            if attr0 & 0x0C00 >= 0x0800 {
                continue;
            }
            let semi_transparent = attr0 & 0x0C00 == 0x0400;
            let color_256 = attr0 & (1 << 13) != 0;
            let shape = (attr0 >> 14) & 0x3;
            let size = (attr1 >> 14) & 0x3;
            let Some((width, height)) = obj_dimensions(shape, size) else {
                continue;
            };
            let double_size = affine && attr0 & (1 << 9) != 0;
            let draw_width = if double_size { width * 2 } else { width };
            let draw_height = if double_size { height * 2 } else { height };
            let obj_y = obj_y_coord(attr0 & 0x00FF);
            if screen_y < obj_y || screen_y >= obj_y + draw_height as i32 {
                continue;
            }
            let obj_x = sign_obj_coord(attr1 & 0x01FF, 512);
            let hflip = !affine && attr1 & (1 << 12) != 0;
            let vflip = !affine && attr1 & (1 << 13) != 0;
            let tile_base = usize::from(attr2 & 0x03FF);
            let obj_priority = ((attr2 >> 10) & 0x3) as u8;
            let palette_bank = usize::from((attr2 >> 12) & 0xF);
            let affine_params = affine.then(|| obj_affine_params(oam, (attr1 >> 9) & 0x1F));
            let use_mosaic = attr0 & (1 << 12) != 0;
            let py = (screen_y - obj_y) as usize;
            for px in 0..draw_width {
                let screen_x = obj_x + px as i32;
                if !(0..SCREEN_WIDTH as i32).contains(&screen_x) {
                    continue;
                }
                let x = screen_x as usize;
                if !windows.allows_obj(x, y) {
                    continue;
                }
                let (sample_px, sample_py) = mosaic.obj_sample(px, py, use_mosaic);
                let Some((src_x, src_y)) = obj_source_pixel(
                    sample_px,
                    sample_py,
                    width,
                    height,
                    draw_width,
                    draw_height,
                    hflip,
                    vflip,
                    affine_params,
                ) else {
                    continue;
                };
                let color_index = obj_color_index(ObjColorParams {
                    vram,
                    tile_base,
                    x: src_x,
                    y: src_y,
                    width,
                    color_256,
                    one_dimensional,
                    bitmap_obj_tiles: dispcnt & 0x7 >= 3,
                });
                if color_index == 0 {
                    continue;
                }
                let palette_index = if color_256 {
                    0x100 + usize::from(color_index)
                } else {
                    0x100 + palette_bank * 16 + usize::from(color_index)
                };
                let color = read_le16(palette_ram, palette_index * 2);
                if obj_priority > obj_priorities[x] {
                    continue;
                }
                if obj_priority > bg_priorities[x] {
                    if obj_priority <= bg_second_priorities[x] {
                        blend_obj_under_current_top_line(
                            &mut self.framebuffer,
                            pixel_layers,
                            pixel_colors,
                            x,
                            y,
                            color,
                            effects,
                            windows.allows_effect(x, y),
                        );
                    }
                    continue;
                }
                obj_priorities[x] = obj_priority;
                draw_color_line(
                    &mut self.framebuffer,
                    pixel_layers,
                    pixel_colors,
                    x,
                    y,
                    color,
                    Layer::Obj,
                    effects,
                    semi_transparent,
                    windows.allows_effect(x, y),
                );
            }
        }
    }
}

fn obj_source_pixel(
    sample_px: usize,
    sample_py: usize,
    width: usize,
    height: usize,
    draw_width: usize,
    draw_height: usize,
    hflip: bool,
    vflip: bool,
    affine_params: Option<(i32, i32, i32, i32)>,
) -> Option<(usize, usize)> {
    if let Some((pa, pb, pc, pd)) = affine_params {
        let rel_x = sample_px as i32 - draw_width as i32 / 2;
        let rel_y = sample_py as i32 - draw_height as i32 / 2;
        let src_x = ((pa * rel_x + pb * rel_y) >> 8) + width as i32 / 2;
        let src_y = ((pc * rel_x + pd * rel_y) >> 8) + height as i32 / 2;
        if !(0..width as i32).contains(&src_x) || !(0..height as i32).contains(&src_y) {
            None
        } else {
            Some((src_x as usize, src_y as usize))
        }
    } else {
        let src_x = if hflip {
            width - 1 - sample_px
        } else {
            sample_px
        };
        let src_y = if vflip {
            height - 1 - sample_py
        } else {
            sample_py
        };
        Some((src_x, src_y))
    }
}

pub(super) fn obj_affine_params(oam: &[u8], index: u16) -> (i32, i32, i32, i32) {
    let base = usize::from(index) * 0x20;
    (
        i32::from(read_i16(oam, base + 0x06)),
        i32::from(read_i16(oam, base + 0x0E)),
        i32::from(read_i16(oam, base + 0x16)),
        i32::from(read_i16(oam, base + 0x1E)),
    )
}

pub(super) fn obj_dimensions(shape: u16, size: u16) -> Option<(usize, usize)> {
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

pub(super) fn sign_obj_coord(value: u16, range: i32) -> i32 {
    let value = i32::from(value);
    if value >= range / 2 {
        value - range
    } else {
        value
    }
}

pub(super) fn obj_y_coord(value: u16) -> i32 {
    let value = i32::from(value & 0x00FF);
    if value >= SCREEN_HEIGHT as i32 {
        value - 256
    } else {
        value
    }
}

pub(super) struct ObjColorParams<'a> {
    pub(super) vram: &'a [u8],
    pub(super) tile_base: usize,
    pub(super) x: usize,
    pub(super) y: usize,
    pub(super) width: usize,
    pub(super) color_256: bool,
    pub(super) one_dimensional: bool,
    pub(super) bitmap_obj_tiles: bool,
}

pub(super) fn obj_color_index(params: ObjColorParams<'_>) -> u16 {
    let ObjColorParams {
        vram,
        tile_base,
        x,
        y,
        width,
        color_256,
        one_dimensional,
        bitmap_obj_tiles,
    } = params;
    let color_stride = if color_256 { 2 } else { 1 };
    let tile_base = if color_256 && !one_dimensional {
        tile_base & !1
    } else {
        tile_base
    };
    let tile_x = x / 8;
    let tile_y = y / 8;
    let tile_number = if one_dimensional {
        let tiles_per_row = width / 8;
        tile_base + (tile_y * tiles_per_row + tile_x) * color_stride
    } else {
        tile_base + tile_y * 32 + tile_x * color_stride
    };
    if bitmap_obj_tiles && tile_number < 512 {
        return 0;
    }
    let base = 0x10000 + tile_number * 32;
    let px = x & 7;
    let py = y & 7;
    if color_256 {
        u16::from(vram.get(base + py * 8 + px).copied().unwrap_or(0))
    } else {
        let byte = vram.get(base + py * 4 + px / 2).copied().unwrap_or(0);
        u16::from(if px & 1 == 0 { byte & 0x0F } else { byte >> 4 })
    }
}

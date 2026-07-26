use super::effects::{ColorEffects, Layer, Mosaic};
use super::window::Windows;
use super::{
    bg_pixel_is_on_top, draw_bg_color, draw_bg_color_line, read_i16, read_i28_8, read_le16,
};
use crate::hardware::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::hardware::ppu::Ppu;

impl Ppu {
    pub(super) fn render_affine_mode(
        &mut self,
        dispcnt: u16,
        io: &[u8],
        palette_ram: &[u8],
        vram: &[u8],
        bg_priorities: &mut [u8],
        bg_layers: &mut [u8],
        bg_second_priorities: &mut [u8],
        bg_second_layers: &mut [Layer],
        bg_second_colors: &mut [u16],
        pixel_layers: &mut [Layer],
        pixel_colors: &mut [u16],
        effects: ColorEffects,
        windows: &Windows,
        mosaic: Mosaic,
    ) {
        self.fill_backdrop(palette_ram, effects, windows, pixel_layers, pixel_colors);
        for bg in 2..=3 {
            if dispcnt & (1 << (8 + bg)) == 0 {
                continue;
            }
            if !self.debug_flags().bg_layers[bg] {
                continue;
            }
            let control = read_le16(io, 0x08 + bg * 2);
            self.render_affine_bg(
                bg,
                control,
                io,
                palette_ram,
                vram,
                bg_priorities,
                bg_layers,
                bg_second_priorities,
                bg_second_layers,
                bg_second_colors,
                pixel_layers,
                pixel_colors,
                effects,
                windows,
                mosaic,
            );
        }
    }

    pub(super) fn render_affine_mode_line(
        &mut self,
        dispcnt: u16,
        io: &[u8],
        palette_ram: &[u8],
        vram: &[u8],
        y: usize,
        bg_priorities: &mut [u8; SCREEN_WIDTH],
        bg_layers: &mut [u8; SCREEN_WIDTH],
        bg_second_priorities: &mut [u8; SCREEN_WIDTH],
        bg_second_layers: &mut [Layer; SCREEN_WIDTH],
        bg_second_colors: &mut [u16; SCREEN_WIDTH],
        pixel_layers: &mut [Layer; SCREEN_WIDTH],
        pixel_colors: &mut [u16; SCREEN_WIDTH],
        effects: ColorEffects,
        windows: &Windows,
        mosaic: Mosaic,
    ) {
        for bg in 2..=3 {
            if dispcnt & (1 << (8 + bg)) == 0 {
                continue;
            }
            if !self.debug_flags().bg_layers[bg] {
                continue;
            }
            let control = read_le16(io, 0x08 + bg * 2);
            self.render_affine_bg_line(
                bg,
                control,
                io,
                palette_ram,
                vram,
                y,
                bg_priorities,
                bg_layers,
                bg_second_priorities,
                bg_second_layers,
                bg_second_colors,
                pixel_layers,
                pixel_colors,
                effects,
                windows,
                mosaic,
            );
        }
    }

    pub(super) fn render_affine_bg(
        &mut self,
        bg: usize,
        control: u16,
        io: &[u8],
        palette_ram: &[u8],
        vram: &[u8],
        bg_priorities: &mut [u8],
        bg_layers: &mut [u8],
        bg_second_priorities: &mut [u8],
        bg_second_layers: &mut [Layer],
        bg_second_colors: &mut [u16],
        pixel_layers: &mut [Layer],
        pixel_colors: &mut [u16],
        effects: ColorEffects,
        windows: &Windows,
        mosaic: Mosaic,
    ) {
        let params = AffineBgParams::new(bg, control, io);
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                if !windows.allows_bg(bg, x, y) {
                    continue;
                }
                let (sample_x, sample_y) = mosaic.bg_sample(x, y, params.use_mosaic);
                let Some((sx, sy)) = params.screen_to_bg(sample_x, sample_y) else {
                    continue;
                };
                let Some(color_index) = params.color_index(vram, sx, sy) else {
                    continue;
                };
                let dst = y * SCREEN_WIDTH + x;
                if !bg_pixel_is_on_top(params.priority, bg, bg_priorities[dst], bg_layers[dst]) {
                    continue;
                }
                draw_bg_color(
                    &mut self.framebuffer,
                    pixel_layers,
                    pixel_colors,
                    bg_priorities,
                    bg_layers,
                    bg_second_priorities,
                    bg_second_layers,
                    bg_second_colors,
                    x,
                    y,
                    read_le16(palette_ram, usize::from(color_index) * 2),
                    bg,
                    params.priority,
                    effects,
                    windows.allows_effect(x, y),
                );
            }
        }
    }

    pub(super) fn render_affine_bg_line(
        &mut self,
        bg: usize,
        control: u16,
        io: &[u8],
        palette_ram: &[u8],
        vram: &[u8],
        y: usize,
        bg_priorities: &mut [u8; SCREEN_WIDTH],
        bg_layers: &mut [u8; SCREEN_WIDTH],
        bg_second_priorities: &mut [u8; SCREEN_WIDTH],
        bg_second_layers: &mut [Layer; SCREEN_WIDTH],
        bg_second_colors: &mut [u16; SCREEN_WIDTH],
        pixel_layers: &mut [Layer; SCREEN_WIDTH],
        pixel_colors: &mut [u16; SCREEN_WIDTH],
        effects: ColorEffects,
        windows: &Windows,
        mosaic: Mosaic,
    ) {
        let params = AffineBgParams::new(bg, control, io);
        for x in 0..SCREEN_WIDTH {
            if !windows.allows_bg(bg, x, y) {
                continue;
            }
            let (sample_x, sample_y) = mosaic.bg_sample(x, y, params.use_mosaic);
            let Some((sx, sy)) = params.screen_to_bg(sample_x, sample_y) else {
                continue;
            };
            let Some(color_index) = params.color_index(vram, sx, sy) else {
                continue;
            };
            if !bg_pixel_is_on_top(params.priority, bg, bg_priorities[x], bg_layers[x]) {
                continue;
            }
            draw_bg_color_line(
                &mut self.framebuffer,
                pixel_layers,
                pixel_colors,
                bg_priorities,
                bg_layers,
                bg_second_priorities,
                bg_second_layers,
                bg_second_colors,
                x,
                y,
                read_le16(palette_ram, usize::from(color_index) * 2),
                bg,
                params.priority,
                effects,
                windows.allows_effect(x, y),
            );
        }
    }
}

struct AffineBgParams {
    char_base: usize,
    priority: u8,
    wrap: bool,
    screen_base: usize,
    size: usize,
    pa: i32,
    pb: i32,
    pc: i32,
    pd: i32,
    ref_x: i32,
    ref_y: i32,
    use_mosaic: bool,
}

impl AffineBgParams {
    fn new(bg: usize, control: u16, io: &[u8]) -> Self {
        let param_base = if bg == 2 { 0x20 } else { 0x30 };
        Self {
            char_base: (((control >> 2) & 0x3) as usize) * 0x4000,
            priority: (control & 0x3) as u8,
            wrap: control & (1 << 13) != 0,
            screen_base: (((control >> 8) & 0x1F) as usize) * 0x800,
            size: 128usize << ((control >> 14) & 0x3),
            pa: i32::from(read_i16(io, param_base)),
            pb: i32::from(read_i16(io, param_base + 2)),
            pc: i32::from(read_i16(io, param_base + 4)),
            pd: i32::from(read_i16(io, param_base + 6)),
            ref_x: read_i28_8(io, param_base + 8),
            ref_y: read_i28_8(io, param_base + 12),
            use_mosaic: control & (1 << 6) != 0,
        }
    }

    fn screen_to_bg(&self, sample_x: usize, sample_y: usize) -> Option<(usize, usize)> {
        let sx = (self.ref_x + self.pa * sample_x as i32 + self.pb * sample_y as i32) >> 8;
        let sy = (self.ref_y + self.pc * sample_x as i32 + self.pd * sample_y as i32) >> 8;
        if self.wrap {
            Some((
                sx.rem_euclid(self.size as i32) as usize,
                sy.rem_euclid(self.size as i32) as usize,
            ))
        } else if sx < 0 || sy < 0 || sx >= self.size as i32 || sy >= self.size as i32 {
            None
        } else {
            Some((sx as usize, sy as usize))
        }
    }

    fn color_index(&self, vram: &[u8], sx: usize, sy: usize) -> Option<u16> {
        let tile_x = sx / 8;
        let tile_y = sy / 8;
        let tiles_per_row = self.size / 8;
        let map_offset = self.screen_base + tile_y * tiles_per_row + tile_x;
        let tile = usize::from(vram.get(map_offset).copied().unwrap_or(0));
        let tile_offset = self.char_base + tile * 64 + (sy & 7) * 8 + (sx & 7);
        let color_index = vram.get(tile_offset).copied().unwrap_or(0);
        if color_index == 0 {
            None
        } else {
            Some(u16::from(color_index))
        }
    }
}

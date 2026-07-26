use super::effects::{ColorEffects, Layer, Mosaic};
use super::window::Windows;
use super::{draw_bg_color, draw_bg_color_line, read_le16};
use crate::hardware::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::hardware::ppu::Ppu;

impl Ppu {
    pub(super) fn render_mode3(
        &mut self,
        io: &[u8],
        palette_ram: &[u8],
        vram: &[u8],
        bg_priorities: &mut [u8],
        bg_layers: &mut [u8],
        bg_second_priorities: &mut [u8],
        bg_second_layers: &mut [Layer],
        bg_second_colors: &mut [u16],
        effects: ColorEffects,
        windows: &Windows,
        pixel_layers: &mut [Layer],
        pixel_colors: &mut [u16],
        mosaic: Mosaic,
    ) {
        self.fill_backdrop(palette_ram, effects, windows, pixel_layers, pixel_colors);
        let priority = (read_le16(io, 0x0C) & 0x3) as u8;
        let use_mosaic = read_le16(io, 0x0C) & (1 << 6) != 0;
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                if !windows.allows_bg(2, x, y) {
                    continue;
                }
                let (sample_x, sample_y) = mosaic.bg_sample(x, y, use_mosaic);
                let src = (sample_y * SCREEN_WIDTH + sample_x) * 2;
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
                    read_le16(vram, src),
                    2,
                    priority,
                    effects,
                    windows.allows_effect(x, y),
                );
            }
        }
    }

    pub(super) fn render_mode4(
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
        effects: ColorEffects,
        windows: &Windows,
        pixel_layers: &mut [Layer],
        pixel_colors: &mut [u16],
        mosaic: Mosaic,
    ) {
        let page = if dispcnt & (1 << 4) != 0 { 0xA000 } else { 0 };
        self.fill_backdrop(palette_ram, effects, windows, pixel_layers, pixel_colors);
        let priority = (read_le16(io, 0x0C) & 0x3) as u8;
        let use_mosaic = read_le16(io, 0x0C) & (1 << 6) != 0;
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                if !windows.allows_bg(2, x, y) {
                    continue;
                }
                let (sample_x, sample_y) = mosaic.bg_sample(x, y, use_mosaic);
                let index = vram
                    .get(page + sample_y * SCREEN_WIDTH + sample_x)
                    .copied()
                    .unwrap_or(0) as usize;
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
                    read_le16(palette_ram, index * 2),
                    2,
                    priority,
                    effects,
                    windows.allows_effect(x, y),
                );
            }
        }
    }

    pub(super) fn render_mode5(
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
        effects: ColorEffects,
        windows: &Windows,
        pixel_layers: &mut [Layer],
        pixel_colors: &mut [u16],
        mosaic: Mosaic,
    ) {
        let page = if dispcnt & (1 << 4) != 0 { 0xA000 } else { 0 };
        self.fill_backdrop(palette_ram, effects, windows, pixel_layers, pixel_colors);
        let priority = (read_le16(io, 0x0C) & 0x3) as u8;
        let use_mosaic = read_le16(io, 0x0C) & (1 << 6) != 0;
        for y in 0..128usize {
            for x in 0..160usize {
                if !windows.allows_bg(2, x, y) {
                    continue;
                }
                let (sample_x, sample_y) = mosaic.bg_sample(x, y, use_mosaic);
                let src = page + (sample_y * 160 + sample_x) * 2;
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
                    read_le16(vram, src),
                    2,
                    priority,
                    effects,
                    windows.allows_effect(x, y),
                );
            }
        }
    }

    pub(super) fn render_mode3_line(
        &mut self,
        io: &[u8],
        _palette_ram: &[u8],
        vram: &[u8],
        y: usize,
        bg_priorities: &mut [u8; SCREEN_WIDTH],
        bg_layers: &mut [u8; SCREEN_WIDTH],
        bg_second_priorities: &mut [u8; SCREEN_WIDTH],
        bg_second_layers: &mut [Layer; SCREEN_WIDTH],
        bg_second_colors: &mut [u16; SCREEN_WIDTH],
        effects: ColorEffects,
        windows: &Windows,
        pixel_layers: &mut [Layer; SCREEN_WIDTH],
        pixel_colors: &mut [u16; SCREEN_WIDTH],
        mosaic: Mosaic,
    ) {
        let priority = (read_le16(io, 0x0C) & 0x3) as u8;
        let use_mosaic = read_le16(io, 0x0C) & (1 << 6) != 0;
        for x in 0..SCREEN_WIDTH {
            if !windows.allows_bg(2, x, y) {
                continue;
            }
            let (sample_x, sample_y) = mosaic.bg_sample(x, y, use_mosaic);
            let src = (sample_y * SCREEN_WIDTH + sample_x) * 2;
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
                read_le16(vram, src),
                2,
                priority,
                effects,
                windows.allows_effect(x, y),
            );
        }
    }

    pub(super) fn render_mode4_line(
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
        effects: ColorEffects,
        windows: &Windows,
        pixel_layers: &mut [Layer; SCREEN_WIDTH],
        pixel_colors: &mut [u16; SCREEN_WIDTH],
        mosaic: Mosaic,
    ) {
        let page = if dispcnt & (1 << 4) != 0 { 0xA000 } else { 0 };
        let priority = (read_le16(io, 0x0C) & 0x3) as u8;
        let use_mosaic = read_le16(io, 0x0C) & (1 << 6) != 0;
        for x in 0..SCREEN_WIDTH {
            if !windows.allows_bg(2, x, y) {
                continue;
            }
            let (sample_x, sample_y) = mosaic.bg_sample(x, y, use_mosaic);
            let index = vram
                .get(page + sample_y * SCREEN_WIDTH + sample_x)
                .copied()
                .unwrap_or(0) as usize;
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
                read_le16(palette_ram, index * 2),
                2,
                priority,
                effects,
                windows.allows_effect(x, y),
            );
        }
    }

    pub(super) fn render_mode5_line(
        &mut self,
        dispcnt: u16,
        io: &[u8],
        _palette_ram: &[u8],
        vram: &[u8],
        y: usize,
        bg_priorities: &mut [u8; SCREEN_WIDTH],
        bg_layers: &mut [u8; SCREEN_WIDTH],
        bg_second_priorities: &mut [u8; SCREEN_WIDTH],
        bg_second_layers: &mut [Layer; SCREEN_WIDTH],
        bg_second_colors: &mut [u16; SCREEN_WIDTH],
        effects: ColorEffects,
        windows: &Windows,
        pixel_layers: &mut [Layer; SCREEN_WIDTH],
        pixel_colors: &mut [u16; SCREEN_WIDTH],
        mosaic: Mosaic,
    ) {
        if y >= 128 {
            return;
        }
        let page = if dispcnt & (1 << 4) != 0 { 0xA000 } else { 0 };
        let priority = (read_le16(io, 0x0C) & 0x3) as u8;
        let use_mosaic = read_le16(io, 0x0C) & (1 << 6) != 0;
        for x in 0..160usize {
            if !windows.allows_bg(2, x, y) {
                continue;
            }
            let (sample_x, sample_y) = mosaic.bg_sample(x, y, use_mosaic);
            let src = page + (sample_y * 160 + sample_x) * 2;
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
                read_le16(vram, src),
                2,
                priority,
                effects,
                windows.allows_effect(x, y),
            );
        }
    }
}

#![allow(clippy::too_many_arguments)]

use super::effects::{ColorEffects, Layer, Mosaic};
use super::window::Windows;
use super::{bg_pixel_is_on_top, draw_bg_color, draw_bg_color_line, read_le16};
use crate::hardware::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::hardware::ppu::Ppu;

impl Ppu {
    pub(super) fn render_text_mode(
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
        bg_count: usize,
        effects: ColorEffects,
        windows: &Windows,
        mosaic: Mosaic,
    ) {
        self.fill_backdrop(palette_ram, effects, windows, pixel_layers, pixel_colors);
        let mut layers = [(0, 0, 0); 4];
        let mut layer_count = 0;
        for bg in 0..bg_count {
            if dispcnt & (1 << (8 + bg)) == 0 {
                continue;
            }
            if !self.debug_flags().bg_layers[bg] {
                continue;
            }
            let control = read_le16(io, 0x08 + bg * 2);
            layers[layer_count] = (control & 0x3, bg, control);
            layer_count += 1;
        }
        layers[..layer_count].sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

        for &(priority, bg, control) in &layers[..layer_count] {
            self.render_text_bg(
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
                priority as u8,
                effects,
                windows,
                mosaic,
            );
        }
    }

    pub(super) fn render_text_mode_line(
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
        bg_count: usize,
        effects: ColorEffects,
        windows: &Windows,
        mosaic: Mosaic,
    ) {
        let mut layers = [(0, 0, 0); 4];
        let mut layer_count = 0;
        for bg in 0..bg_count {
            if dispcnt & (1 << (8 + bg)) == 0 {
                continue;
            }
            if !self.debug_flags().bg_layers[bg] {
                continue;
            }
            let control = read_le16(io, 0x08 + bg * 2);
            layers[layer_count] = (control & 0x3, bg, control);
            layer_count += 1;
        }
        layers[..layer_count].sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

        for &(priority, bg, control) in &layers[..layer_count] {
            self.render_text_bg_line(
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
                priority as u8,
                effects,
                windows,
                mosaic,
            );
        }
    }

    fn render_text_bg(
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
        priority: u8,
        effects: ColorEffects,
        windows: &Windows,
        mosaic: Mosaic,
    ) {
        let params = TextBgParams::new(control, io, bg);
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                if !windows.allows_bg(bg, x, y) {
                    continue;
                }
                let (sample_x, sample_y) = mosaic.bg_sample(x, y, params.use_mosaic);
                let sx = (sample_x + params.hofs) % params.width;
                let sy = (sample_y + params.vofs) % params.height;
                let Some(color_index) = params.color_index(vram, sx, sy) else {
                    continue;
                };
                if color_index == 0 {
                    continue;
                }
                let dst = y * SCREEN_WIDTH + x;
                if !bg_pixel_is_on_top(priority, bg, bg_priorities[dst], bg_layers[dst]) {
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
                    priority,
                    effects,
                    windows.allows_effect(x, y),
                );
            }
        }
    }

    fn render_text_bg_line(
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
        priority: u8,
        effects: ColorEffects,
        windows: &Windows,
        mosaic: Mosaic,
    ) {
        let params = TextBgParams::new(control, io, bg);
        let mut tile_entry = TextTileEntryCache::default();
        let cache_tile_entries = windows.effects_are_uniform();
        for x in 0..SCREEN_WIDTH {
            if !windows.allows_bg(bg, x, y) {
                continue;
            }
            let (sample_x, sample_y) = mosaic.bg_sample(x, y, params.use_mosaic);
            let sx = (sample_x + params.hofs) % params.width;
            let sy = (sample_y + params.vofs) % params.height;
            let color_index = if cache_tile_entries {
                params.color_index_cached(vram, sx, sy, &mut tile_entry)
            } else {
                params.color_index(vram, sx, sy)
            };
            let Some(color_index) = color_index else {
                continue;
            };
            if color_index == 0 {
                continue;
            }
            if !bg_pixel_is_on_top(priority, bg, bg_priorities[x], bg_layers[x]) {
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
                priority,
                effects,
                windows.allows_effect(x, y),
            );
        }
    }
}

struct TextTileEntryCache {
    offset: usize,
    entry: u16,
}

impl Default for TextTileEntryCache {
    fn default() -> Self {
        Self {
            offset: usize::MAX,
            entry: 0,
        }
    }
}

impl TextTileEntryCache {
    fn read(&mut self, vram: &[u8], offset: usize) -> u16 {
        if self.offset != offset {
            self.offset = offset;
            self.entry = read_le16(vram, offset);
        }
        self.entry
    }
}

struct TextBgParams {
    char_base: usize,
    screen_base: usize,
    width: usize,
    height: usize,
    color_256: bool,
    hofs: usize,
    vofs: usize,
    use_mosaic: bool,
}

#[cfg(test)]
mod tests {
    use super::{TextBgParams, TextTileEntryCache};

    #[test]
    fn cached_tile_rows_match_per_pixel_lookup() {
        let mut vram = vec![0; 0x1_8000];
        for (i, byte) in vram.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(37).wrapping_add((i >> 8) as u8);
        }
        let io = [0; 0x60];

        for size in 0..4 {
            for color_256 in [false, true] {
                let control = (size << 14) | (0x1A << 8) | if color_256 { 1 << 7 } else { 0 };
                let params = TextBgParams::new(control, &io, 0);
                let mut cache = TextTileEntryCache::default();
                for y in 0..params.height {
                    for x in 0..params.width {
                        assert_eq!(
                            params.color_index_cached(&vram, x, y, &mut cache),
                            params.color_index(&vram, x, y),
                            "size={size} color_256={color_256} x={x} y={y}"
                        );
                    }
                }
            }
        }
    }
}

impl TextBgParams {
    fn new(control: u16, io: &[u8], bg: usize) -> Self {
        let size = (control >> 14) & 0x3;
        let (width, height) = match size {
            0 => (256usize, 256usize),
            1 => (512, 256),
            2 => (256, 512),
            _ => (512, 512),
        };
        Self {
            char_base: (((control >> 2) & 0x3) as usize) * 0x4000,
            screen_base: (((control >> 8) & 0x1F) as usize) * 0x800,
            width,
            height,
            color_256: control & (1 << 7) != 0,
            hofs: usize::from(read_le16(io, 0x10 + bg * 4)) & 0x1FF,
            vofs: usize::from(read_le16(io, 0x12 + bg * 4)) & 0x1FF,
            use_mosaic: control & (1 << 6) != 0,
        }
    }

    fn entry_offset(&self, x: usize, y: usize) -> usize {
        let screen_x = x / 256;
        let screen_y = y / 256;
        let block = match (self.width > 256, screen_y > 0, screen_x > 0) {
            (false, true, _) => 1,
            (true, false, true) => 1,
            (true, true, false) => 2,
            (true, true, true) => 3,
            _ => 0,
        };
        let tile_x = (x % 256) / 8;
        let tile_y = (y % 256) / 8;
        self.screen_base + block * 0x800 + (tile_y * 32 + tile_x) * 2
    }

    fn color_index(&self, vram: &[u8], x: usize, y: usize) -> Option<u16> {
        let entry = read_le16(vram, self.entry_offset(x, y));
        self.color_index_from_entry(vram, x, y, entry)
    }

    fn color_index_cached(
        &self,
        vram: &[u8],
        x: usize,
        y: usize,
        cache: &mut TextTileEntryCache,
    ) -> Option<u16> {
        let entry = cache.read(vram, self.entry_offset(x, y));
        self.color_index_from_entry(vram, x, y, entry)
    }

    fn color_index_from_entry(&self, vram: &[u8], x: usize, y: usize, entry: u16) -> Option<u16> {
        let tile = usize::from(entry & 0x03FF);
        let hflip = entry & (1 << 10) != 0;
        let vflip = entry & (1 << 11) != 0;
        let palette_bank = usize::from((entry >> 12) & 0xF);
        let px = if hflip { 7 - (x & 7) } else { x & 7 };
        let py = if vflip { 7 - (y & 7) } else { y & 7 };
        if self.color_256 {
            let tile_offset = self.char_base + tile * 64 + py * 8 + px;
            let color = vram.get(tile_offset).copied().unwrap_or(0);
            if color == 0 {
                None
            } else {
                Some(u16::from(color))
            }
        } else {
            let tile_offset = self.char_base + tile * 32 + py * 4 + px / 2;
            let byte = vram.get(tile_offset).copied().unwrap_or(0);
            let nibble = if px & 1 == 0 { byte & 0x0F } else { byte >> 4 };
            if nibble == 0 {
                None
            } else {
                Some((palette_bank * 16 + usize::from(nibble)) as u16)
            }
        }
    }
}

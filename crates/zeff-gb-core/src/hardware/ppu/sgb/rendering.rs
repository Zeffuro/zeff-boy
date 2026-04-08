use std::sync::atomic::{AtomicBool, Ordering};

use super::super::{
    PPU, SCREEN_H, SCREEN_W, SGB_BORDER_H, SGB_BORDER_PALETTES, SGB_BORDER_TILEMAP_W, SGB_BORDER_W,
};
use super::rgb555_to_rgba;

static SGB_EMPTY_BORDER_WARNED: AtomicBool = AtomicBool::new(false);

impl PPU {
    pub fn render_sgb_border_framebuffer(&mut self) {
        if !self.sgb_border_enabled || !self.sgb_enabled {
            return;
        }

        let game_fb = &self.framebuffer;
        let border_fb = &mut self.sgb_composite_buffer;
        border_fb.fill(0);

        let palettes_non_zero = self
            .sgb_border_palettes
            .iter()
            .flat_map(|p| p.iter())
            .any(|&c| c != 0);
        let tilemap_non_zero = self.sgb_border_tilemap.iter().any(|&e| e != 0);
        if !palettes_non_zero || !tilemap_non_zero {
            if !SGB_EMPTY_BORDER_WARNED.swap(true, Ordering::Relaxed) {
                log::warn!(
                    "SGB border render has empty state: tilemap_non_zero={}, palettes_non_zero={} (border may appear black)",
                    tilemap_non_zero,
                    palettes_non_zero
                );
            }
        } else {
            SGB_EMPTY_BORDER_WARNED.store(false, Ordering::Relaxed);
        }

        for tile_y in 0..(SGB_BORDER_H / 8) {
            for tile_x in 0..SGB_BORDER_TILEMAP_W {
                let map_idx = tile_y * SGB_BORDER_TILEMAP_W + tile_x;
                if map_idx >= self.sgb_border_tilemap.len() {
                    continue;
                }

                let entry = self.sgb_border_tilemap[map_idx];
                let tile_count = self.sgb_border_tile_data.len() / 32;
                if tile_count == 0 {
                    continue;
                }
                let tile_idx = ((entry & 0x03FF) as usize) % tile_count;
                let palette_idx = ((entry >> 10) & 0x07) as usize;
                let x_flip = entry & 0x4000 != 0;
                let y_flip = entry & 0x8000 != 0;

                let palette = &self.sgb_border_palettes[palette_idx % SGB_BORDER_PALETTES];

                for tile_row in 0..8 {
                    for tile_col in 0..8 {
                        let screen_x = tile_x * 8 + tile_col;
                        let screen_y = tile_y * 8 + tile_row;

                        if screen_x >= SGB_BORDER_W || screen_y >= SGB_BORDER_H {
                            continue;
                        }

                        let local_x = if x_flip { 7 - tile_col } else { tile_col };
                        let local_y = if y_flip { 7 - tile_row } else { tile_row };

                        let color_id = decode_snes_4bpp_color(
                            &self.sgb_border_tile_data,
                            tile_idx,
                            local_x,
                            local_y,
                        );

                        if color_id >= 16 {
                            continue;
                        }

                        let color = palette[color_id as usize];
                        let rgba = rgb555_to_rgba(color);

                        let offset = (screen_y * SGB_BORDER_W + screen_x) * 4;
                        border_fb[offset..offset + 4].copy_from_slice(&rgba);
                    }
                }
            }
        }

        let game_x_offset = (SGB_BORDER_W - SCREEN_W) / 2;
        let game_y_offset = (SGB_BORDER_H - SCREEN_H) / 2;

        for game_y in 0..SCREEN_H {
            for game_x in 0..SCREEN_W {
                let src_offset = (game_y * SCREEN_W + game_x) * 4;
                let dst_x = game_x_offset + game_x;
                let dst_y = game_y_offset + game_y;
                let dst_offset = (dst_y * SGB_BORDER_W + dst_x) * 4;

                border_fb[dst_offset..dst_offset + 4]
                    .copy_from_slice(&game_fb[src_offset..src_offset + 4]);
            }
        }
    }
}

fn decode_snes_4bpp_color(tile_data: &[u8], tile_index: usize, x: usize, y: usize) -> u8 {
    let tile_base = tile_index * 32;
    let row = y & 0x07;
    let bit = 7 - (x & 0x07);

    let p0_idx = tile_base + row * 2;
    let p1_idx = p0_idx + 1;
    let p2_idx = tile_base + 16 + row * 2;
    let p3_idx = p2_idx + 1;

    if p3_idx >= tile_data.len() {
        return 0;
    }

    let p0 = (tile_data[p0_idx] >> bit) & 1;
    let p1 = (tile_data[p1_idx] >> bit) & 1;
    let p2 = (tile_data[p2_idx] >> bit) & 1;
    let p3 = (tile_data[p3_idx] >> bit) & 1;

    p0 | (p1 << 1) | (p2 << 2) | (p3 << 3)
}

use crate::hardware::ppu::palette::apply_palette;
use crate::hardware::ppu::{PPU, SCREEN_H, SCREEN_W, SpriteEntry};

fn render_bg_pixel(
    vram: &[u8],
    tile_map_base: usize,
    tile_data_unsigned: bool,
    bg_x: usize,
    bg_y: usize,
) -> u8 {
    let tile_row = bg_y / 8;
    let tile_col = bg_x / 8;
    let tile_map_addr = tile_map_base + tile_row * 32 + tile_col;
    let tile_index = vram.get(tile_map_addr).copied().unwrap_or(0);

    let tile_data_addr = if tile_data_unsigned {
        (tile_index as usize) * 16
    } else {
        ((tile_index as i8 as i16 + 128) as usize) * 16
    };

    let line_in_tile = bg_y % 8;
    let byte_offset = tile_data_addr + line_in_tile * 2;
    let lo = vram.get(byte_offset).copied().unwrap_or(0);
    let hi = vram.get(byte_offset + 1).copied().unwrap_or(0);

    let bit = 7 - (bg_x % 8) as u8;
    ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1)
}

fn render_window_pixel(
    vram: &[u8],
    tile_map_base: usize,
    tile_data_unsigned: bool,
    wx_offset: usize,
    wy_offset: usize,
) -> u8 {
    let tile_row = wy_offset / 8;
    let tile_col = wx_offset / 8;
    let tile_map_addr = tile_map_base + tile_row * 32 + tile_col;
    let tile_index = vram.get(tile_map_addr).copied().unwrap_or(0);

    let tile_data_addr = if tile_data_unsigned {
        (tile_index as usize) * 16
    } else {
        ((tile_index as i8 as i16 + 128) as usize) * 16
    };

    let line_in_tile = wy_offset % 8;
    let byte_offset = tile_data_addr + line_in_tile * 2;
    let lo = vram.get(byte_offset).copied().unwrap_or(0);
    let hi = vram.get(byte_offset + 1).copied().unwrap_or(0);

    let bit = 7 - (wx_offset % 8) as u8;
    ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1)
}

fn render_sprites(
    lcdc: u8,
    bgp: u8,
    obp0: u8,
    obp1: u8,
    vram: &[u8],
    oam: &[u8],
    ly: usize,
    framebuffer: &mut [u8],
) {
    if lcdc & 0x02 == 0 {
        return;
    }

    let tall_sprites = lcdc & 0x04 != 0;
    let sprite_height: u8 = if tall_sprites { 16 } else { 8 };

    let mut sprites_on_line: Vec<SpriteEntry> = Vec::with_capacity(10);

    for i in 0..40 {
        let sprite = SpriteEntry::from_oam(oam, i);
        let sy = sprite.y;

        if (ly as i32) >= sy && (ly as i32) < sy + sprite_height as i32 {
            sprites_on_line.push(sprite);
            if sprites_on_line.len() >= 10 {
                break;
            }
        }
    }

    sprites_on_line.sort_by(|a, b| a.x.cmp(&b.x).then(a.oam_index.cmp(&b.oam_index)));

    for sprite in sprites_on_line.iter().rev() {
        let palette = if sprite.palette_number() == 1 {
            obp1
        } else {
            obp0
        };

        let flip_x = sprite.flip_x();
        let flip_y = sprite.flip_y();
        let bg_priority = sprite.bg_priority();

        let mut line_in_sprite = (ly as i32 - sprite.y) as usize;
        let tile_index = if tall_sprites {
            let base_tile = sprite.tile & 0xFE;
            if flip_y {
                line_in_sprite = 15 - line_in_sprite;
            }
            if line_in_sprite >= 8 {
                base_tile + 1
            } else {
                base_tile
            }
        } else {
            if flip_y {
                line_in_sprite = 7 - line_in_sprite;
            }
            sprite.tile
        };

        let tile_line = line_in_sprite % 8;
        let tile_addr = (tile_index as usize) * 16 + tile_line * 2;
        let lo = vram.get(tile_addr).copied().unwrap_or(0);
        let hi = vram.get(tile_addr + 1).copied().unwrap_or(0);

        for px in 0..8 {
            let screen_x = sprite.x + px as i32;
            if screen_x < 0 || screen_x >= SCREEN_W as i32 {
                continue;
            }

            let bit = if flip_x { px } else { 7 - px };
            let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);

            if color_id == 0 {
                continue; // transparent
            }

            // BG priority: if set, sprite only shows over BG color 0
            if bg_priority {
                let fb_offset = (ly * SCREEN_W + screen_x as usize) * 4;
                // Check if BG pixel is color 0 (lightest)
                let bg_shade = apply_palette(bgp, 0);
                if framebuffer[fb_offset..fb_offset + 4] != bg_shade {
                    continue;
                }
            }

            let rgba = apply_palette(palette, color_id);
            let fb_offset = (ly * SCREEN_W + screen_x as usize) * 4;
            framebuffer[fb_offset..fb_offset + 4].copy_from_slice(&rgba);
        }
    }
}
pub(crate) fn render_scanline(ppu: &mut PPU, vram: &[u8], oam: &[u8]) {
    let ly = ppu.ly as usize;
    if ly >= SCREEN_H {
        return;
    }

    let bg_enabled = ppu.lcdc & 0x01 != 0;

    let bg_tile_map_base: usize = if ppu.lcdc & 0x08 != 0 { 0x1C00 } else { 0x1800 };

    let tile_data_unsigned = ppu.lcdc & 0x10 != 0;

    let scroll_y = ppu.scy as usize;
    let scroll_x = ppu.scx as usize;

    let window_enabled = ppu.lcdc & 0x20 != 0;
    let win_tile_map_base: usize = if ppu.lcdc & 0x40 != 0 { 0x1C00 } else { 0x1800 };
    let win_x = ppu.wx as i32 - 7;
    let win_y = ppu.wy as usize;
    let window_visible = window_enabled && ly >= win_y && win_x < SCREEN_W as i32;

    let mut window_used_this_line = false;

    for x in 0..SCREEN_W {
        let mut color_id: u8 = 0;

        if window_visible && (x as i32) >= win_x {
            let wx_offset = (x as i32 - win_x) as usize;
            let wy_offset = ppu.window_line_counter as usize;
            color_id = render_window_pixel(
                vram,
                win_tile_map_base,
                tile_data_unsigned,
                wx_offset,
                wy_offset,
            );
            window_used_this_line = true;
        } else if bg_enabled {
            let bg_y = (ly + scroll_y) & 0xFF;
            let bg_x = (x + scroll_x) & 0xFF;
            color_id = render_bg_pixel(vram, bg_tile_map_base, tile_data_unsigned, bg_x, bg_y);
        }

        let rgba = apply_palette(ppu.bgp, color_id);
        let offset = (ly * SCREEN_W + x) * 4;
        ppu.framebuffer[offset..offset + 4].copy_from_slice(&rgba);
    }

    if window_used_this_line {
        ppu.window_line_counter += 1;
    }

    render_sprites(
        ppu.lcdc,
        ppu.bgp,
        ppu.obp0,
        ppu.obp1,
        vram,
        oam,
        ly,
        &mut ppu.framebuffer,
    );
}


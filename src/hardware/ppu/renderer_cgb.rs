use super::{SpriteRenderContext, decode_cgb_tile_attributes, render_sprites};
use crate::hardware::ppu::{PPU, SCREEN_H, SCREEN_W, decode_tile_pixel, tile_data_address};

pub(crate) fn render_scanline_cgb(ppu: &mut PPU, vram: &[u8], oam: &[u8]) {
    let ly = ppu.ly as usize;
    if ly >= SCREEN_H {
        return;
    }

    let bg_tile_map_base: usize = if ppu.lcdc & 0x08 != 0 { 0x1C00 } else { 0x1800 };
    let win_tile_map_base: usize = if ppu.lcdc & 0x40 != 0 { 0x1C00 } else { 0x1800 };
    let tile_data_unsigned = ppu.lcdc & 0x10 != 0;
    let mut bg_color_ids = [0u8; SCREEN_W];
    let mut bg_priority_flags = [false; SCREEN_W];

    for x in 0..SCREEN_W {
        let (map_base, map_x, map_y, is_window) = {
            let win_x = ppu.wx as i32 - 7;
            if ppu.debug_enable_window
                && ppu.window_visible_on_current_line()
                && win_x < SCREEN_W as i32
                && (x as i32) >= win_x
            {
                (
                    win_tile_map_base,
                    (x as i32 - win_x) as usize,
                    ppu.window_line_counter as usize,
                    true,
                )
            } else {
                (
                    bg_tile_map_base,
                    (x + ppu.scx as usize) & 0xFF,
                    (ly + ppu.scy as usize) & 0xFF,
                    false,
                )
            }
        };

        if !ppu.debug_enable_bg && !is_window {
            let offset = (ly * SCREEN_W + x) * 4;
            ppu.framebuffer[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            bg_color_ids[x] = 0;
            bg_priority_flags[x] = false;
            continue;
        }

        let tile_row = map_y / 8;
        let tile_col = map_x / 8;
        let tile_map_addr = map_base + tile_row * 32 + tile_col;

        let tile_index = vram.get(tile_map_addr).copied().unwrap_or(0);
        let attr_addr = 0x2000 + tile_map_addr;
        let attrs = decode_cgb_tile_attributes(vram.get(attr_addr).copied().unwrap_or(0));

        let tile_data_addr = tile_data_address(tile_index, tile_data_unsigned);
        let line_in_tile = map_y % 8;
        let pixel_in_tile = map_x % 8;
        let source_line = if attrs.flip_y {
            7 - line_in_tile
        } else {
            line_in_tile
        };
        let source_pixel = if attrs.flip_x {
            7 - pixel_in_tile
        } else {
            pixel_in_tile
        };

        let banked_tile_addr = attrs.vram_bank * 0x2000 + tile_data_addr;
        let color_id = decode_tile_pixel(vram, banked_tile_addr, source_line, source_pixel);
        bg_color_ids[x] = color_id;
        bg_priority_flags[x] = attrs.bg_to_oam_priority;

        let rgba = ppu.cgb_bg_rgba(attrs.bg_palette, color_id);
        let offset = (ly * SCREEN_W + x) * 4;
        ppu.framebuffer[offset..offset + 4].copy_from_slice(&rgba);
    }

    ppu.increment_window_line_counter_after_scanline();

    if ppu.debug_enable_sprites {
        render_sprites(SpriteRenderContext {
            cgb_mode: true,
            lcdc: ppu.lcdc,
            obp0: ppu.obp0,
            obp1: ppu.obp1,
            vram,
            oam,
            ly,
            framebuffer: &mut ppu.framebuffer,
            cgb_obj_palette_ram: Some(&ppu.obj_palette_ram),
            bg_color_ids: Some(&bg_color_ids),
            cgb_bg_priority_flags: Some(&bg_priority_flags),
            color_correction: ppu.color_correction,
            color_correction_matrix: ppu.color_correction_matrix,
        });
    }
}

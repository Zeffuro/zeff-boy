use super::{SpriteRenderContext, render_sprites};
use crate::hardware::ppu::palette::apply_palette;
use crate::hardware::ppu::{
    LCDC_BG_ENABLE, LCDC_BG_TILEMAP, LCDC_TILE_DATA, LCDC_WINDOW_TILEMAP, PPU, SCREEN_H, SCREEN_W,
    decode_tile_pixel, tile_data_address,
};

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

    let tile_data_addr = tile_data_address(tile_index, tile_data_unsigned);
    decode_tile_pixel(vram, tile_data_addr, bg_y % 8, bg_x % 8)
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

    let tile_data_addr = tile_data_address(tile_index, tile_data_unsigned);
    decode_tile_pixel(vram, tile_data_addr, wy_offset % 8, wx_offset % 8)
}

fn render_window_line(
    ppu: &PPU,
    vram: &[u8],
    tile_data_unsigned: bool,
    win_tile_map_base: usize,
    x: usize,
    window_visible: bool,
) -> Option<u8> {
    let win_x = ppu.wx as i32 - 7;

    if !window_visible || win_x >= SCREEN_W as i32 || (x as i32) < win_x {
        return None;
    }

    let wx_offset = (x as i32 - win_x) as usize;
    let wy_offset = ppu.window_line_counter as usize;
    Some(render_window_pixel(
        vram,
        win_tile_map_base,
        tile_data_unsigned,
        wx_offset,
        wy_offset,
    ))
}

fn render_bg_line(
    ppu: &PPU,
    vram: &[u8],
    tile_data_unsigned: bool,
    bg_tile_map_base: usize,
    ly: usize,
    x: usize,
) -> u8 {
    if ppu.lcdc & LCDC_BG_ENABLE == 0 {
        return 0;
    }
    let bg_y = (ly + ppu.scy as usize) & 0xFF;
    let bg_x = (x + ppu.scx as usize) & 0xFF;
    render_bg_pixel(vram, bg_tile_map_base, tile_data_unsigned, bg_x, bg_y)
}

pub fn render_scanline_dmg(ppu: &mut PPU, vram: &[u8], oam: &[u8]) {
    let ly = ppu.ly as usize;
    if ly >= SCREEN_H {
        return;
    }

    if ppu.sgb_enabled {
        match ppu.sgb_mask_mode {
            1 => {
                return;
            }
            2 | 3 => {
                for x in 0..SCREEN_W {
                    let offset = (ly * SCREEN_W + x) * 4;
                    ppu.framebuffer[offset..offset + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
                return;
            }
            _ => {}
        }
    }

    let bg_tile_map_base: usize = if ppu.lcdc & LCDC_BG_TILEMAP != 0 {
        0x1C00
    } else {
        0x1800
    };

    let tile_data_unsigned = ppu.lcdc & LCDC_TILE_DATA != 0;
    let win_tile_map_base: usize = if ppu.lcdc & LCDC_WINDOW_TILEMAP != 0 {
        0x1C00
    } else {
        0x1800
    };

    let mut bg_color_ids = [0u8; SCREEN_W];
    let window_visible = ppu.window_visible_on_current_line();

    for (x, bg_color_id) in bg_color_ids.iter_mut().enumerate() {
        let color_id = if ppu.debug_flags.window {
            render_window_line(
                ppu,
                vram,
                tile_data_unsigned,
                win_tile_map_base,
                x,
                window_visible,
            )
        } else {
            None
        };

        let color_id = color_id.unwrap_or_else(|| {
            if ppu.debug_flags.bg {
                render_bg_line(ppu, vram, tile_data_unsigned, bg_tile_map_base, ly, x)
            } else {
                0
            }
        });

        *bg_color_id = color_id;

        let rgba = apply_palette(ppu.bgp, color_id);
        let offset = (ly * SCREEN_W + x) * 4;
        ppu.framebuffer[offset..offset + 4].copy_from_slice(&rgba);
    }


    if ppu.debug_flags.sprites {
        render_sprites(SpriteRenderContext {
            cgb_mode: false,
            lcdc: ppu.lcdc,
            obp0: ppu.obp0,
            obp1: ppu.obp1,
            vram,
            oam,
            ly,
            framebuffer: &mut ppu.framebuffer,
            cgb_obj_palette_ram: None,
            bg_color_ids: Some(&bg_color_ids),
            cgb_bg_priority_flags: None,
        });
    }

    if ppu.sgb_enabled {
        for x in 0..SCREEN_W {
            let offset = (ly * SCREEN_W + x) * 4;
            let rgba = [
                ppu.framebuffer[offset],
                ppu.framebuffer[offset + 1],
                ppu.framebuffer[offset + 2],
                ppu.framebuffer[offset + 3],
            ];
            let mapped = ppu.sgb_remap_dmg_rgba(rgba);
            ppu.framebuffer[offset..offset + 4].copy_from_slice(&mapped);
        }
    }
}

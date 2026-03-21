use crate::hardware::ppu::palette::{apply_palette, cgb_palette_rgba};
use crate::hardware::ppu::{PPU, SCREEN_H, SCREEN_W, SpriteEntry};
use crate::hardware::ppu::{decode_tile_pixel, tile_data_address};

#[derive(Clone, Copy)]
struct CgbTileAttributes {
    bg_palette: u8,
    vram_bank: usize,
    flip_x: bool,
    flip_y: bool,
    bg_to_oam_priority: bool,
}

fn decode_cgb_tile_attributes(attr: u8) -> CgbTileAttributes {
    CgbTileAttributes {
        bg_palette: attr & 0x07,
        vram_bank: ((attr >> 3) & 0x01) as usize,
        flip_x: attr & 0x20 != 0,
        flip_y: attr & 0x40 != 0,
        bg_to_oam_priority: attr & 0x80 != 0,
    }
}

fn cgb_sprite_hidden_by_bg(
    lcdc: u8,
    sprite_bg_priority: bool,
    bg_color_id: u8,
    bg_to_oam_priority: bool,
) -> bool {
    if lcdc & 0x01 == 0 {
        return false;
    }
    bg_color_id != 0 && (sprite_bg_priority || bg_to_oam_priority)
}

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
    ly: usize,
    x: usize,
) -> Option<u8> {
    let window_enabled = ppu.lcdc & 0x20 != 0;
    let win_x = ppu.wx as i32 - 7;
    let win_y = ppu.wy as usize;
    if !window_enabled || ly < win_y || win_x >= SCREEN_W as i32 || (x as i32) < win_x {
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
    if ppu.lcdc & 0x01 == 0 {
        return 0;
    }
    let bg_y = (ly + ppu.scy as usize) & 0xFF;
    let bg_x = (x + ppu.scx as usize) & 0xFF;
    render_bg_pixel(vram, bg_tile_map_base, tile_data_unsigned, bg_x, bg_y)
}

fn render_sprites(
    cgb_mode: bool,
    lcdc: u8,
    obp0: u8,
    obp1: u8,
    vram: &[u8],
    oam: &[u8],
    ly: usize,
    framebuffer: &mut [u8],
    cgb_obj_palette_ram: Option<&[u8; 64]>,
    bg_color_ids: Option<&[u8; SCREEN_W]>,
    cgb_bg_priority_flags: Option<&[bool; SCREEN_W]>,
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

    sprites_on_line.sort_by(|a, b| {
        if cgb_mode {
            a.oam_index.cmp(&b.oam_index)
        } else {
            a.x.cmp(&b.x).then(a.oam_index.cmp(&b.oam_index))
        }
    });

    for sprite in sprites_on_line.iter().rev() {
        let dmg_palette = if sprite.palette_number() == 1 { obp1 } else { obp0 };

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
        let banked_tile_addr = if cgb_mode {
            sprite.cgb_vram_bank() * 0x2000 + tile_addr
        } else {
            tile_addr
        };
        let lo = vram.get(banked_tile_addr).copied().unwrap_or(0);
        let hi = vram.get(banked_tile_addr + 1).copied().unwrap_or(0);

        for px in 0..8 {
            let screen_x = sprite.x + px as i32;
            if screen_x < 0 || screen_x >= SCREEN_W as i32 {
                continue;
            }

            let bit = if flip_x { px } else { 7 - px };
            let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);

            if color_id == 0 {
                continue;
            }

            let screen_x_usize = screen_x as usize;

            if let (Some(bg_color_ids), Some(bg_priority_flags)) =
                (bg_color_ids, cgb_bg_priority_flags)
            {
                if cgb_sprite_hidden_by_bg(
                    lcdc,
                    bg_priority,
                    bg_color_ids[screen_x_usize],
                    bg_priority_flags[screen_x_usize],
                ) {
                    continue;
                }
            } else if bg_priority {
                if bg_color_ids.expect("dmg bg color ids provided")[screen_x_usize] != 0 {
                    continue;
                }
            }

            let rgba = if cgb_mode {
                let obj_palette_ram = cgb_obj_palette_ram.expect("cgb obj palette ram provided");
                cgb_palette_rgba(obj_palette_ram, sprite.cgb_obj_palette_index(), color_id)
            } else {
                apply_palette(dmg_palette, color_id)
            };
            let fb_offset = (ly * SCREEN_W + screen_x_usize) * 4;
            framebuffer[fb_offset..fb_offset + 4].copy_from_slice(&rgba);
        }
    }
}
pub(crate) fn render_scanline_dmg(ppu: &mut PPU, vram: &[u8], oam: &[u8]) {
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

    let bg_tile_map_base: usize = if ppu.lcdc & 0x08 != 0 { 0x1C00 } else { 0x1800 };

    let tile_data_unsigned = ppu.lcdc & 0x10 != 0;
    let win_tile_map_base: usize = if ppu.lcdc & 0x40 != 0 { 0x1C00 } else { 0x1800 };

    let mut window_used_this_line = false;
    let mut bg_color_ids = [0u8; SCREEN_W];

    for x in 0..SCREEN_W {
        let color_id = if let Some(window_color) =
            render_window_line(ppu, vram, tile_data_unsigned, win_tile_map_base, ly, x)
        {
            window_used_this_line = true;
            window_color
        } else {
            render_bg_line(ppu, vram, tile_data_unsigned, bg_tile_map_base, ly, x)
        };

        bg_color_ids[x] = color_id;

        let rgba = apply_palette(ppu.bgp, color_id);
        let offset = (ly * SCREEN_W + x) * 4;
        ppu.framebuffer[offset..offset + 4].copy_from_slice(&rgba);
    }

    if window_used_this_line {
        ppu.window_line_counter += 1;
    }

    render_sprites(
        false,
        ppu.lcdc,
        ppu.obp0,
        ppu.obp1,
        vram,
        oam,
        ly,
        &mut ppu.framebuffer,
        None,
        Some(&bg_color_ids),
        None,
    );

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

pub(crate) fn render_scanline_cgb(ppu: &mut PPU, vram: &[u8], oam: &[u8]) {
    let ly = ppu.ly as usize;
    if ly >= SCREEN_H {
        return;
    }

    let bg_tile_map_base: usize = if ppu.lcdc & 0x08 != 0 { 0x1C00 } else { 0x1800 };
    let win_tile_map_base: usize = if ppu.lcdc & 0x40 != 0 { 0x1C00 } else { 0x1800 };
    let tile_data_unsigned = ppu.lcdc & 0x10 != 0;
    let mut window_used_this_line = false;
    let mut bg_color_ids = [0u8; SCREEN_W];
    let mut bg_priority_flags = [false; SCREEN_W];

    for x in 0..SCREEN_W {
        let (map_base, map_x, map_y, is_window) = {
            let window_enabled = ppu.lcdc & 0x20 != 0;
            let win_x = ppu.wx as i32 - 7;
            let win_y = ppu.wy as usize;
            if window_enabled && ly >= win_y && win_x < SCREEN_W as i32 && (x as i32) >= win_x {
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

        if is_window {
            window_used_this_line = true;
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
        let source_line = if attrs.flip_y { 7 - line_in_tile } else { line_in_tile };
        let source_pixel = if attrs.flip_x { 7 - pixel_in_tile } else { pixel_in_tile };

        let banked_tile_addr = attrs.vram_bank * 0x2000 + tile_data_addr;
        let color_id = decode_tile_pixel(vram, banked_tile_addr, source_line, source_pixel);
        bg_color_ids[x] = color_id;
        bg_priority_flags[x] = attrs.bg_to_oam_priority;

        let rgba = ppu.cgb_bg_rgba(attrs.bg_palette, color_id);
        let offset = (ly * SCREEN_W + x) * 4;
        ppu.framebuffer[offset..offset + 4].copy_from_slice(&rgba);
    }

    if window_used_this_line {
        ppu.window_line_counter += 1;
    }

    render_sprites(
        true,
        ppu.lcdc,
        ppu.obp0,
        ppu.obp1,
        vram,
        oam,
        ly,
        &mut ppu.framebuffer,
        Some(&ppu.obj_palette_ram),
        Some(&bg_color_ids),
        Some(&bg_priority_flags),
    );
}

#[cfg(test)]
mod tests {
    use super::cgb_sprite_hidden_by_bg;

    #[test]
    fn cgb_bg_attr_priority_blocks_sprite_on_non_zero_bg() {
        assert!(cgb_sprite_hidden_by_bg(0x91, false, 2, true));
    }

    #[test]
    fn cgb_sprite_priority_flag_blocks_sprite_on_non_zero_bg() {
        assert!(cgb_sprite_hidden_by_bg(0x91, true, 1, false));
    }

    #[test]
    fn cgb_allows_sprite_when_bg_color_zero() {
        assert!(!cgb_sprite_hidden_by_bg(0x91, true, 0, true));
    }

    #[test]
    fn cgb_lcdc_bg_priority_disable_allows_sprite_over_bg() {
        assert!(!cgb_sprite_hidden_by_bg(0x90, true, 3, true));
    }
}


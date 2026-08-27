use super::{SpriteRenderContext, decode_cgb_tile_attributes, render_sprites};
use crate::hardware::ppu::palette::cgb_palette_rgba;
use crate::hardware::ppu::tiles::{decode_tile_row_pixel, read_tile_row};
use crate::hardware::ppu::{Lcdc, PPU, SCREEN_H, SCREEN_W, tile_data_address};

struct CgbLineRenderer<'a> {
    vram: &'a [u8],
    tile_data_unsigned: bool,
    palettes: [[[u8; 4]; 4]; 8],
    framebuffer: &'a mut [u8],
    color_ids: &'a mut [u8; SCREEN_W],
    priority_flags: &'a mut [bool; SCREEN_W],
}

impl CgbLineRenderer<'_> {
    fn fill_white(&mut self) {
        for x in 0..SCREEN_W {
            self.color_ids[x] = 0;
            self.priority_flags[x] = false;
            self.framebuffer[x * 4..x * 4 + 4].copy_from_slice(&[255; 4]);
        }
    }

    fn render_tiles(
        &mut self,
        range: std::ops::Range<usize>,
        tile_map_base: usize,
        mut map_x: usize,
        map_y: usize,
        wrap_x: bool,
    ) {
        let mut screen_x = range.start;
        while screen_x < range.end {
            let tile_map_addr = tile_map_base + (map_y / 8) * 32 + map_x / 8;
            let tile_index = self.vram.get(tile_map_addr).copied().unwrap_or(0);
            let attrs = decode_cgb_tile_attributes(
                self.vram.get(0x2000 + tile_map_addr).copied().unwrap_or(0),
            );
            let line_in_tile = map_y % 8;
            let source_line = if attrs.flip_y {
                7 - line_in_tile
            } else {
                line_in_tile
            };
            let tile_data_addr = tile_data_address(tile_index, self.tile_data_unsigned);
            let banked_tile_addr = attrs.vram_bank * 0x2000 + tile_data_addr;
            let row = read_tile_row(self.vram, banked_tile_addr, source_line);
            let pixels = (8 - map_x % 8).min(range.end - screen_x);

            for offset in 0..pixels {
                let pixel_in_tile = map_x % 8 + offset;
                let source_pixel = if attrs.flip_x {
                    7 - pixel_in_tile
                } else {
                    pixel_in_tile
                };
                let color_id = decode_tile_row_pixel(row, source_pixel);
                let x = screen_x + offset;
                self.color_ids[x] = color_id;
                self.priority_flags[x] = attrs.bg_to_oam_priority;
                let rgba = self.palettes[attrs.bg_palette as usize][color_id as usize];
                self.framebuffer[x * 4..x * 4 + 4].copy_from_slice(&rgba);
            }

            screen_x += pixels;
            map_x += pixels;
            if wrap_x {
                map_x &= 0xFF;
            }
        }
    }
}

pub fn render_scanline_cgb(ppu: &mut PPU, vram: &[u8], oam: &[u8]) {
    let ly = ppu.ly as usize;
    if ly >= SCREEN_H {
        return;
    }

    let bg_tile_map_base: usize = if ppu.lcdc.contains(Lcdc::BG_TILEMAP) {
        0x1C00
    } else {
        0x1800
    };
    let win_tile_map_base: usize = if ppu.lcdc.contains(Lcdc::WINDOW_TILEMAP) {
        0x1C00
    } else {
        0x1800
    };
    let tile_data_unsigned = ppu.lcdc.contains(Lcdc::TILE_DATA);
    let mut bg_color_ids = [0u8; SCREEN_W];
    let mut bg_priority_flags = [false; SCREEN_W];
    let window_visible = ppu.window_visible_on_current_line();
    let palettes = std::array::from_fn(|palette| {
        std::array::from_fn(|color| {
            cgb_palette_rgba(&ppu.bg_palette_ram, palette as u8, color as u8)
        })
    });
    let line_offset = ly * SCREEN_W * 4;
    let mut renderer = CgbLineRenderer {
        vram,
        tile_data_unsigned,
        palettes,
        framebuffer: &mut ppu.framebuffer[line_offset..line_offset + SCREEN_W * 4],
        color_ids: &mut bg_color_ids,
        priority_flags: &mut bg_priority_flags,
    };

    if ppu.debug_flags.bg {
        renderer.render_tiles(
            0..SCREEN_W,
            bg_tile_map_base,
            ppu.scx as usize,
            (ly + ppu.scy as usize) & 0xFF,
            true,
        );
    } else {
        renderer.fill_white();
    }

    let window_x = ppu.wx as i32 - 7;
    if ppu.debug_flags.window && window_visible && window_x < SCREEN_W as i32 {
        let screen_x = window_x.max(0) as usize;
        renderer.render_tiles(
            screen_x..SCREEN_W,
            win_tile_map_base,
            (screen_x as i32 - window_x) as usize,
            ppu.window_line_counter as usize,
            false,
        );
    }

    if ppu.debug_flags.sprites {
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
            dmg_palette_preset: ppu.dmg_palette_preset,
            selected_obj_indices: (!ppu.legacy_sprite_selection_for_line)
                .then_some((ppu.selected_obj_indices, ppu.selected_obj_count)),
            selected_obj_tile_rows: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterned_bytes(len: usize, mut value: u32) -> Vec<u8> {
        (0..len)
            .map(|_| {
                value = value.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (value >> 24) as u8
            })
            .collect()
    }

    fn reference_pixel(
        vram: &[u8],
        tile_map_base: usize,
        tile_data_unsigned: bool,
        x: usize,
        y: usize,
    ) -> (u8, bool, usize) {
        let tile_map_addr = tile_map_base + (y / 8) * 32 + x / 8;
        let tile_index = vram[tile_map_addr];
        let attr = vram[0x2000 + tile_map_addr];
        let line = if attr & 0x40 != 0 { 7 - y % 8 } else { y % 8 };
        let pixel = if attr & 0x20 != 0 { 7 - x % 8 } else { x % 8 };
        let tile_addr = (usize::from(attr & 0x08 != 0) * 0x2000)
            + tile_data_address(tile_index, tile_data_unsigned)
            + line * 2;
        let bit = 7 - pixel as u8;
        let color_id = ((vram[tile_addr + 1] >> bit) & 1) << 1 | ((vram[tile_addr] >> bit) & 1);
        (color_id, attr & 0x80 != 0, (attr & 0x07) as usize)
    }

    #[test]
    fn tile_spans_match_per_pixel_reference() {
        let vram = patterned_bytes(0x4000, 0xC001_C0DE);
        let palette_ram: [u8; 64] = patterned_bytes(64, 0x1234_5678).try_into().unwrap();
        let palettes = std::array::from_fn(|palette| {
            std::array::from_fn(|color| cgb_palette_rgba(&palette_ram, palette as u8, color as u8))
        });

        for tile_data_unsigned in [false, true] {
            for tile_map_base in [0x1800, 0x1C00] {
                for map_y in [0, 7, 8, 127, 255] {
                    for map_x in [0, 1, 7, 8, 249, 255] {
                        let mut framebuffer = [0; SCREEN_W * 4];
                        let mut color_ids = [0; SCREEN_W];
                        let mut priority_flags = [false; SCREEN_W];
                        CgbLineRenderer {
                            vram: &vram,
                            tile_data_unsigned,
                            palettes,
                            framebuffer: &mut framebuffer,
                            color_ids: &mut color_ids,
                            priority_flags: &mut priority_flags,
                        }
                        .render_tiles(
                            0..SCREEN_W,
                            tile_map_base,
                            map_x,
                            map_y,
                            true,
                        );

                        for x in 0..SCREEN_W {
                            let (color_id, priority, palette) = reference_pixel(
                                &vram,
                                tile_map_base,
                                tile_data_unsigned,
                                (map_x + x) & 0xFF,
                                map_y,
                            );
                            assert_eq!(color_ids[x], color_id);
                            assert_eq!(priority_flags[x], priority);
                            assert_eq!(
                                framebuffer[x * 4..x * 4 + 4],
                                palettes[palette][color_id as usize]
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn partial_window_span_keeps_screen_and_map_offsets_distinct() {
        let vram = patterned_bytes(0x4000, 0xC001_C0DE);
        let palette_ram: [u8; 64] = patterned_bytes(64, 0x1234_5678).try_into().unwrap();
        let palettes = std::array::from_fn(|palette| {
            std::array::from_fn(|color| cgb_palette_rgba(&palette_ram, palette as u8, color as u8))
        });
        let mut framebuffer = [0xA5; SCREEN_W * 4];
        let mut color_ids = [0xFF; SCREEN_W];
        let mut priority_flags = [true; SCREEN_W];
        CgbLineRenderer {
            vram: &vram,
            tile_data_unsigned: false,
            palettes,
            framebuffer: &mut framebuffer,
            color_ids: &mut color_ids,
            priority_flags: &mut priority_flags,
        }
        .render_tiles(3..SCREEN_W, 0x1800, 5, 23, false);

        assert_eq!(color_ids[..3], [0xFF; 3]);
        for screen_x in 3..SCREEN_W {
            let (color_id, priority, palette) =
                reference_pixel(&vram, 0x1800, false, 5 + screen_x - 3, 23);
            assert_eq!(color_ids[screen_x], color_id);
            assert_eq!(priority_flags[screen_x], priority);
            assert_eq!(
                framebuffer[screen_x * 4..screen_x * 4 + 4],
                palettes[palette][color_id as usize]
            );
        }
    }
}

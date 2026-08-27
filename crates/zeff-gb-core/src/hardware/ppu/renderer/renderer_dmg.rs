use super::{SpriteRenderContext, render_sprites};
use crate::hardware::ppu::palette::apply_dmg_palette;
use crate::hardware::ppu::tiles::{decode_tile_row_pixel, read_tile_row};
use crate::hardware::ppu::{Lcdc, PPU, SCREEN_H, SCREEN_W, SGB_ATTR_BLOCKS_W, tile_data_address};

struct DmgLineRenderer<'a> {
    vram: &'a [u8],
    tile_data_unsigned: bool,
    palette: [[u8; 4]; 4],
    framebuffer: &'a mut [u8],
    color_ids: &'a mut [u8; SCREEN_W],
}

impl DmgLineRenderer<'_> {
    fn fill(&mut self, range: std::ops::Range<usize>, color_id: u8) {
        let rgba = self.palette[color_id as usize];
        for x in range {
            self.color_ids[x] = color_id;
            self.framebuffer[x * 4..x * 4 + 4].copy_from_slice(&rgba);
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
            let tile_data_addr = tile_data_address(tile_index, self.tile_data_unsigned);
            let row = read_tile_row(self.vram, tile_data_addr, map_y % 8);
            let pixels = (8 - map_x % 8).min(range.end - screen_x);

            for offset in 0..pixels {
                let color_id = decode_tile_row_pixel(row, map_x % 8 + offset);
                let x = screen_x + offset;
                self.color_ids[x] = color_id;
                let rgba = self.palette[color_id as usize];
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

    let bg_tile_map_base: usize = if ppu.lcdc.contains(Lcdc::BG_TILEMAP) {
        0x1C00
    } else {
        0x1800
    };

    let tile_data_unsigned = ppu.lcdc.contains(Lcdc::TILE_DATA);
    let win_tile_map_base: usize = if ppu.lcdc.contains(Lcdc::WINDOW_TILEMAP) {
        0x1C00
    } else {
        0x1800
    };

    let mut bg_color_ids = [0u8; SCREEN_W];
    let window_visible = ppu.window_visible_on_current_line();
    let line_offset = ly * SCREEN_W * 4;
    let palette = std::array::from_fn(|color_id| {
        apply_dmg_palette(ppu.dmg_palette_preset, ppu.bgp, color_id as u8)
    });
    let mut renderer = DmgLineRenderer {
        vram,
        tile_data_unsigned,
        palette,
        framebuffer: &mut ppu.framebuffer[line_offset..line_offset + SCREEN_W * 4],
        color_ids: &mut bg_color_ids,
    };

    if ppu.debug_flags.bg && ppu.lcdc.contains(Lcdc::BG_ENABLE) {
        renderer.render_tiles(
            0..SCREEN_W,
            bg_tile_map_base,
            ppu.scx as usize,
            (ly + ppu.scy as usize) & 0xFF,
            true,
        );
    } else {
        renderer.fill(0..SCREEN_W, 0);
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
            dmg_palette_preset: ppu.dmg_palette_preset,
            selected_obj_indices: (!ppu.legacy_sprite_selection_for_line)
                .then_some((ppu.selected_obj_indices, ppu.selected_obj_count)),
            selected_obj_tile_rows: (!ppu.legacy_sprite_selection_for_line
                && !ppu.legacy_obj_fetch_for_line)
                .then_some((
                    ppu.mode3_obj_tile_rows,
                    ppu.mode3_obj_tile_row_latched_mask,
                    ppu.mode3_obj_completed_mask,
                )),
        });
    }

    if ppu.sgb_enabled {
        let tile_y = ly / 8;
        for x in 0..SCREEN_W {
            let attr_idx = tile_y * SGB_ATTR_BLOCKS_W + (x / 8);
            let palette_idx = ppu.sgb_attr_map.get(attr_idx).copied().unwrap_or(0) as usize;
            let offset = (ly * SCREEN_W + x) * 4;
            let rgba = [
                ppu.framebuffer[offset],
                ppu.framebuffer[offset + 1],
                ppu.framebuffer[offset + 2],
                ppu.framebuffer[offset + 3],
            ];
            let mapped = ppu.sgb_remap_pixel(rgba, palette_idx);
            ppu.framebuffer[offset..offset + 4].copy_from_slice(&mapped);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::ppu::DmgPalettePreset;

    fn patterned_vram() -> Vec<u8> {
        let mut value = 0xA5A5_5A5A_u32;
        (0..0x4000)
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
    ) -> u8 {
        let tile_map_addr = tile_map_base + (y / 8) * 32 + x / 8;
        let tile_index = vram[tile_map_addr];
        let tile_data_addr = tile_data_address(tile_index, tile_data_unsigned) + (y % 8) * 2;
        let bit = 7 - (x % 8) as u8;
        ((vram[tile_data_addr + 1] >> bit) & 1) << 1 | ((vram[tile_data_addr] >> bit) & 1)
    }

    #[test]
    fn tile_spans_match_per_pixel_reference() {
        let vram = patterned_vram();
        let palette_byte = 0b00_01_10_11;
        let palette = std::array::from_fn(|color_id| {
            apply_dmg_palette(DmgPalettePreset::DmgGreen, palette_byte, color_id as u8)
        });

        for tile_data_unsigned in [false, true] {
            for tile_map_base in [0x1800, 0x1C00] {
                for map_y in [0, 7, 8, 127, 255] {
                    for map_x in [0, 1, 7, 8, 249, 255] {
                        let mut framebuffer = [0; SCREEN_W * 4];
                        let mut color_ids = [0; SCREEN_W];
                        DmgLineRenderer {
                            vram: &vram,
                            tile_data_unsigned,
                            palette,
                            framebuffer: &mut framebuffer,
                            color_ids: &mut color_ids,
                        }
                        .render_tiles(
                            0..SCREEN_W,
                            tile_map_base,
                            map_x,
                            map_y,
                            true,
                        );

                        for x in 0..SCREEN_W {
                            let color_id = reference_pixel(
                                &vram,
                                tile_map_base,
                                tile_data_unsigned,
                                (map_x + x) & 0xFF,
                                map_y,
                            );
                            assert_eq!(color_ids[x], color_id);
                            assert_eq!(framebuffer[x * 4..x * 4 + 4], palette[color_id as usize]);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn partial_window_span_keeps_screen_and_map_offsets_distinct() {
        let vram = patterned_vram();
        let palette = std::array::from_fn(|color_id| {
            apply_dmg_palette(DmgPalettePreset::Gray, 0b11_10_01_00, color_id as u8)
        });
        let mut framebuffer = [0xA5; SCREEN_W * 4];
        let mut color_ids = [0xFF; SCREEN_W];
        DmgLineRenderer {
            vram: &vram,
            tile_data_unsigned: true,
            palette,
            framebuffer: &mut framebuffer,
            color_ids: &mut color_ids,
        }
        .render_tiles(3..SCREEN_W, 0x1C00, 5, 23, false);

        assert_eq!(color_ids[..3], [0xFF; 3]);
        for screen_x in 3..SCREEN_W {
            let color_id = reference_pixel(&vram, 0x1C00, true, 5 + screen_x - 3, 23);
            assert_eq!(color_ids[screen_x], color_id);
            assert_eq!(
                framebuffer[screen_x * 4..screen_x * 4 + 4],
                palette[color_id as usize]
            );
        }
    }
}

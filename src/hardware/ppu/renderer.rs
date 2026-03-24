use crate::hardware::ppu::palette::{apply_palette, cgb_palette_rgba};
use crate::hardware::ppu::{SCREEN_W, SpriteEntry};

#[path = "renderer_cgb.rs"]
mod cgb;
#[path = "renderer_dmg.rs"]
mod dmg;

pub(crate) use cgb::render_scanline_cgb;
pub(crate) use dmg::render_scanline_dmg;

#[derive(Clone, Copy)]
pub(super) struct CgbTileAttributes {
    pub(super) bg_palette: u8,
    pub(super) vram_bank: usize,
    pub(super) flip_x: bool,
    pub(super) flip_y: bool,
    pub(super) bg_to_oam_priority: bool,
}

pub(super) fn decode_cgb_tile_attributes(attr: u8) -> CgbTileAttributes {
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

pub(super) fn render_sprites(
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
    color_correction: crate::settings::ColorCorrection,
    color_correction_matrix: [f32; 9],
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
        let dmg_palette = if sprite.palette_number() == 1 {
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
                cgb_palette_rgba(
                    obj_palette_ram,
                    sprite.cgb_obj_palette_index(),
                    color_id,
                    color_correction,
                    color_correction_matrix,
                )
            } else {
                apply_palette(dmg_palette, color_id)
            };
            let fb_offset = (ly * SCREEN_W + screen_x_usize) * 4;
            framebuffer[fb_offset..fb_offset + 4].copy_from_slice(&rgba);
        }
    }
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

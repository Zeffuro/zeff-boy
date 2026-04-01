use super::{PPU, apply_dmg_palette, dmg_palette_colors, SGB_ATTR_BLOCKS_H, SGB_ATTR_BLOCKS_W, SGB_ATTR_FILE_SIZE, SGB_BORDER_H, SGB_BORDER_W, SGB_CHR_TRANSFER_SIZE, SCREEN_H, SCREEN_W};

impl PPU {
    pub fn set_sgb_mode(&mut self, enabled: bool) {
        self.sgb_enabled = enabled;
    }

    pub fn set_sgb_palette(&mut self, index: usize, colors: [u16; 4]) {
        if index < 4 {
            self.sgb_palettes[index] = colors;
        }
    }

    pub fn set_sgb_active_palette(&mut self, index: u8) {
        self.sgb_active_palette = index & 0x03;
    }

    pub fn set_sgb_mask_mode(&mut self, mode: u8) {
        self.sgb_mask_mode = mode & 0x03;
    }

    pub fn set_sgb_border_enabled(&mut self, enabled: bool) {
        self.sgb_border_enabled = enabled;
    }

    pub fn sgb_dmg_rgba(&self, dmg_palette: u8, color_id: u8) -> [u8; 4] {
        if !self.sgb_enabled {
            return apply_dmg_palette(self.dmg_palette_preset, dmg_palette, color_id);
        }
        let shade = ((dmg_palette >> (color_id * 2)) & 0x03) as usize;
        rgb555_to_rgba(self.sgb_palettes[self.sgb_active_palette as usize][shade])
    }

    pub fn sgb_remap_dmg_rgba(&self, rgba: [u8; 4]) -> [u8; 4] {
        if !self.sgb_enabled {
            return rgba;
        }
        for (shade, dmg_color) in dmg_palette_colors(self.dmg_palette_preset)
            .iter()
            .enumerate()
        {
            if *dmg_color == rgba {
                return rgb555_to_rgba(self.sgb_palettes[self.sgb_active_palette as usize][shade]);
            }
        }
        rgba
    }

    pub fn sgb_chr_trn(&mut self, vram: &[u8], vram_bank: u8) {
        let src_addr = if vram_bank == 0 { 0x0000 } else { 0x2000 };
        let len = SGB_CHR_TRANSFER_SIZE.min(vram.len().saturating_sub(src_addr));
        self.sgb_border_tile_data[..len].copy_from_slice(&vram[src_addr..src_addr + len]);
    }

    pub fn sgb_attr_trn(&mut self, vram: &[u8], vram_bank: u8) {
        let src_addr = if vram_bank == 0 { 0x0000 } else { 0x2000 };
        let len = SGB_ATTR_FILE_SIZE.min(vram.len().saturating_sub(src_addr));
        self.sgb_attr_file[..len].copy_from_slice(&vram[src_addr..src_addr + len]);
    }

    pub fn sgb_attr_set(&mut self, map_base: u8, palette_idx: u8) {
        self.sgb_attr_map_base = map_base & 0x3F;
        self.sgb_attr_palette[0] = palette_idx & 0x03;
    }

    pub fn render_sgb_border_framebuffer(&mut self) {
        if !self.sgb_border_enabled || !self.sgb_enabled {
            return;
        }

        let game_fb = &self.framebuffer;
        let border_fb = &mut self.sgb_composite_buffer;

        for block_y in 0..SGB_ATTR_BLOCKS_H {
            for block_x in 0..SGB_ATTR_BLOCKS_W {
                let attr_idx = block_y * SGB_ATTR_BLOCKS_W + block_x;
                if attr_idx >= self.sgb_attr_file.len() {
                    continue;
                }

                let attr_byte = self.sgb_attr_file[attr_idx];
                let palette_idx = (attr_byte >> 2) & 0x03;
                let tile_idx = attr_byte & 0x03;

                let palette = self.sgb_palettes[palette_idx as usize];

                for tile_row in 0..8 {
                    for tile_col in 0..8 {
                        let screen_x = block_x * 8 + tile_col;
                        let screen_y = block_y * 8 + tile_row;

                        if screen_x >= SGB_BORDER_W || screen_y >= SGB_BORDER_H {
                            continue;
                        }

                        let tile_data_idx = (tile_idx as usize) * 16 + tile_row * 2;
                        if tile_data_idx + 1 >= self.sgb_border_tile_data.len() {
                            continue;
                        }

                        let lo = self.sgb_border_tile_data[tile_data_idx];
                        let hi = self.sgb_border_tile_data[tile_data_idx + 1];

                        let bit = 7 - tile_col;
                        let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);

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

    pub fn sgb_border_active(&self) -> bool {
        self.sgb_border_enabled && self.sgb_enabled
    }

    pub fn sgb_composite_buffer(&self) -> &[u8] {
        &self.sgb_composite_buffer
    }

    pub fn reset_framebuffer_for_rendering(&mut self) {
        if self.framebuffer.len() != SCREEN_W * SCREEN_H * 4 {
            self.framebuffer = vec![0; SCREEN_W * SCREEN_H * 4].into_boxed_slice();
        }
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        if self.sgb_border_active() {
            (SGB_BORDER_W, SGB_BORDER_H)
        } else {
            (SCREEN_W, SCREEN_H)
        }
    }
}

fn rgb555_to_rgba(color: u16) -> [u8; 4] {
    let r5 = (color & 0x1F) as u8;
    let g5 = ((color >> 5) & 0x1F) as u8;
    let b5 = ((color >> 10) & 0x1F) as u8;
    let expand = |v: u8| (v << 3) | (v >> 2);
    [expand(r5), expand(g5), expand(b5), 255]
}

mod commands;
mod rendering;

use super::{PPU, SCREEN_H, SCREEN_W, SGB_BORDER_H, SGB_BORDER_W, dmg_palette_colors};

impl PPU {
    pub fn set_sgb_mode(&mut self, enabled: bool) {
        self.sgb_enabled = enabled;
    }

    pub fn set_sgb_palette(&mut self, index: usize, colors: [u16; 4]) {
        if index < 4 {
            self.sgb_palettes[index] = colors;
        }
    }

    pub fn sgb_pal_set(&mut self, pal_indices: [u16; 4], attr_file: u8, cancel_mask: bool) {
        for (i, &idx) in pal_indices.iter().enumerate() {
            let base = (idx as usize) * 8;
            if base + 7 < self.sgb_pal_trn_data.len() {
                let mut colors = [0u16; 4];
                for (c, color) in colors.iter_mut().enumerate() {
                    let lo = self.sgb_pal_trn_data[base + c * 2] as u16;
                    let hi = self.sgb_pal_trn_data[base + c * 2 + 1] as u16;
                    *color = lo | (hi << 8);
                }
                self.sgb_palettes[i] = colors;
            }
        }
        if cancel_mask {
            self.sgb_mask_mode = 0;
        }
        log::info!(
            "SGB PAL_SET applied: indices={:?}, attr_file={}, cancel_mask={}",
            pal_indices,
            attr_file,
            cancel_mask
        );
    }

    pub fn set_sgb_mask_mode(&mut self, mode: u8) {
        self.sgb_mask_mode = mode & 0x03;
    }

    pub fn set_sgb_border_enabled(&mut self, enabled: bool) {
        self.sgb_border_enabled = enabled;
    }

    pub fn sgb_remap_pixel(&self, rgba: [u8; 4], palette_idx: usize) -> [u8; 4] {
        for (shade, dmg_color) in dmg_palette_colors(self.dmg_palette_preset)
            .iter()
            .enumerate()
        {
            if *dmg_color == rgba {
                return rgb555_to_rgba(self.sgb_palettes[palette_idx & 3][shade]);
            }
        }
        rgba
    }

    pub fn sgb_remap_dmg_rgba(&self, rgba: [u8; 4]) -> [u8; 4] {
        if !self.sgb_enabled {
            return rgba;
        }
        self.sgb_remap_pixel(rgba, self.sgb_active_palette as usize)
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

pub(super) fn rgb555_to_rgba(color: u16) -> [u8; 4] {
    let r5 = (color & 0x1F) as u8;
    let g5 = ((color >> 5) & 0x1F) as u8;
    let b5 = ((color >> 10) & 0x1F) as u8;
    let expand = |v: u8| (v << 3) | (v >> 2);
    [expand(r5), expand(g5), expand(b5), 255]
}

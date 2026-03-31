use super::{PPU, apply_dmg_palette, dmg_palette_colors};

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
}

fn rgb555_to_rgba(color: u16) -> [u8; 4] {
    let r5 = (color & 0x1F) as u8;
    let g5 = ((color >> 5) & 0x1F) as u8;
    let b5 = ((color >> 10) & 0x1F) as u8;
    let expand = |v: u8| (v << 3) | (v >> 2);
    [expand(r5), expand(g5), expand(b5), 255]
}

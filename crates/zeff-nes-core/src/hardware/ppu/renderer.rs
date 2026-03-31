use super::Ppu;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NesPaletteMode {
    #[default]
    Raw,
    Ntsc,
    Pal,
}

#[inline]
fn scale_u8(v: u8, num: u16, den: u16) -> u8 {
    ((u16::from(v) * num) / den).min(255) as u8
}

#[inline]
pub fn apply_nes_palette_mode(mode: NesPaletteMode, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let (r, g, b) = rgb;
    match mode {
        NesPaletteMode::Raw => (r, g, b),
        NesPaletteMode::Ntsc => {
            let r_out = scale_u8(r, 246, 255);
            let g_out = scale_u8(g, 250, 255);
            let b_out = scale_u8(b, 242, 255);
            (r_out, g_out, b_out)
        }
        NesPaletteMode::Pal => {
            let r_out = scale_u8(r, 236, 255);
            let g_out = scale_u8(g, 244, 255);
            let b_out = scale_u8(b, 255, 255);
            (r_out, g_out, b_out)
        }
    }
}

#[rustfmt::skip]
pub static NES_PALETTE: [(u8, u8, u8); 64] = [
    (84,84,84),    (0,30,116),    (8,16,144),    (48,0,136),
    (68,0,100),    (92,0,48),     (84,4,0),      (60,24,0),
    (32,42,0),     (8,58,0),      (0,64,0),      (0,60,0),
    (0,50,60),     (0,0,0),       (0,0,0),       (0,0,0),

    (152,150,152), (8,76,196),    (48,50,236),   (92,30,228),
    (136,20,176),  (160,20,100),  (152,34,32),   (120,60,0),
    (84,90,0),     (40,114,0),    (8,124,0),     (0,118,40),
    (0,102,120),   (0,0,0),       (0,0,0),       (0,0,0),

    (236,238,236), (76,154,236),  (120,124,236), (176,98,236),
    (228,84,236),  (236,88,180),  (236,106,100), (212,136,32),
    (160,170,0),   (116,196,0),   (76,208,32),   (56,204,108),
    (56,180,204),  (60,60,60),    (0,0,0),       (0,0,0),

    (236,238,236), (168,204,236), (188,188,236), (212,178,236),
    (236,174,236), (236,174,212), (236,180,176), (228,196,144),
    (204,210,120), (180,222,120), (168,226,144), (152,226,180),
    (160,214,228), (160,162,160), (0,0,0),       (0,0,0),
];

impl Ppu {
    #[inline]
    pub fn compose_pixel(&mut self) -> u8 {
        let x = self.dot.wrapping_sub(1) as u8;

        let mut bg_pixel: u8 = 0;
        let mut bg_palette: u8 = 0;

        if self.regs.show_bg() && (x >= 8 || self.regs.show_bg_left8()) {
            let mux = 0x8000u16 >> self.fine_x;
            let p0 = ((self.bg_shift_pattern_lo & mux) != 0) as u8;
            let p1 = ((self.bg_shift_pattern_hi & mux) != 0) as u8;
            bg_pixel = (p1 << 1) | p0;

            let a0 = ((self.bg_shift_attrib_lo & mux) != 0) as u8;
            let a1 = ((self.bg_shift_attrib_hi & mux) != 0) as u8;
            bg_palette = (a1 << 1) | a0;
        }

        let mut spr_pixel: u8 = 0;
        let mut spr_palette: u8 = 0;
        let mut spr_priority = false;
        let mut sprite_zero_hit = false;

        if self.regs.show_sprites() && (x >= 8 || self.regs.show_sprites_left8()) {
            for i in 0..self.sprite_count as usize {
                if self.sprite_x_counters[i] == 0 {
                    let p0 = ((self.sprite_patterns_lo[i] & 0x80) != 0) as u8;
                    let p1 = ((self.sprite_patterns_hi[i] & 0x80) != 0) as u8;
                    let pixel = (p1 << 1) | p0;

                    if pixel != 0 {
                        spr_pixel = pixel;
                        spr_palette = (self.sprite_attribs[i] & 0x03) + 4;
                        spr_priority = self.sprite_attribs[i] & 0x20 != 0;

                        if i == 0 && self.sprite_zero_rendering {
                            sprite_zero_hit = true;
                        }
                        break;
                    }
                }
            }
        }

        let (pixel, palette) = match (bg_pixel != 0, spr_pixel != 0) {
            (false, false) => (0u8, 0u8),
            (false, true) => (spr_pixel, spr_palette),
            (true, false) => (bg_pixel, bg_palette),
            (true, true) => {
                if sprite_zero_hit && x < 255 {
                    self.regs.set_sprite_zero_hit();
                }
                if !spr_priority {
                    (spr_pixel, spr_palette)
                } else {
                    (bg_pixel, bg_palette)
                }
            }
        };

        if pixel == 0 {
            self.palette_ram[0] & 0x3F
        } else {
            let addr = (palette as usize) * 4 + pixel as usize;
            self.palette_ram[addr & 0x1F] & 0x3F
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NesPaletteMode, apply_nes_palette_mode};

    #[test]
    fn raw_mode_is_identity() {
        assert_eq!(
            apply_nes_palette_mode(NesPaletteMode::Raw, (100, 150, 200)),
            (100, 150, 200)
        );
    }

    #[test]
    fn ntsc_and_pal_modes_produce_distinct_results() {
        let src = (180, 120, 90);
        let ntsc = apply_nes_palette_mode(NesPaletteMode::Ntsc, src);
        let pal = apply_nes_palette_mode(NesPaletteMode::Pal, src);
        assert_ne!(ntsc, src);
        assert_ne!(pal, src);
        assert_ne!(ntsc, pal);
    }
}

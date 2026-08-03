use super::Ppu;
use anyhow::{bail, ensure};
use serde::{Deserialize, Serialize};

pub const NES_PALETTE_COLOR_COUNT: usize = 64;
pub const NES_PALETTE_RGB_BYTES: usize = NES_PALETTE_COLOR_COUNT * 3;
pub const NES_PALETTE_EMPHASIS_GROUPS: usize = 8;
pub const NES_PALETTE_EMPHASIS_RGB_BYTES: usize = NES_PALETTE_RGB_BYTES * 8;

pub type NesBasePalette = [(u8, u8, u8); NES_PALETTE_COLOR_COUNT];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NesPalette {
    Base(NesBasePalette),
    WithEmphasis([NesBasePalette; NES_PALETTE_EMPHASIS_GROUPS]),
}

impl NesPalette {
    pub fn base(self) -> NesBasePalette {
        match self {
            Self::Base(palette) => palette,
            Self::WithEmphasis(palettes) => palettes[0],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NesPaletteMode {
    #[default]
    Raw,
    Ntsc,
    Pal,
    Custom,
}

#[inline]
fn scale_u8(v: u8, num: u16, den: u16) -> u8 {
    ((u16::from(v) * num) / den).min(255) as u8
}

#[inline]
pub fn apply_nes_palette_mode(mode: NesPaletteMode, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let (r, g, b) = rgb;
    match mode {
        NesPaletteMode::Raw | NesPaletteMode::Custom => (r, g, b),
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

pub fn parse_nes_palette_bytes(bytes: &[u8]) -> anyhow::Result<NesPalette> {
    ensure!(
        bytes.len() == NES_PALETTE_RGB_BYTES || bytes.len() == NES_PALETTE_EMPHASIS_RGB_BYTES,
        "NES palette must be {NES_PALETTE_RGB_BYTES} bytes (64 RGB colors) or {NES_PALETTE_EMPHASIS_RGB_BYTES} bytes (64 RGB colors x 8 emphasis groups), got {} bytes",
        bytes.len()
    );

    if bytes.len() == NES_PALETTE_RGB_BYTES {
        return Ok(NesPalette::Base(parse_nes_base_palette(bytes)?));
    }

    let mut palettes = [[(0u8, 0u8, 0u8); NES_PALETTE_COLOR_COUNT]; NES_PALETTE_EMPHASIS_GROUPS];
    for (group, palette) in palettes.iter_mut().enumerate() {
        let offset = group * NES_PALETTE_RGB_BYTES;
        let Some(raw_palette) = bytes.get(offset..offset + NES_PALETTE_RGB_BYTES) else {
            bail!("NES palette is missing emphasis group {group}");
        };
        *palette = parse_nes_base_palette(raw_palette)?;
    }

    Ok(NesPalette::WithEmphasis(palettes))
}

fn parse_nes_base_palette(bytes: &[u8]) -> anyhow::Result<NesBasePalette> {
    ensure!(
        bytes.len() == NES_PALETTE_RGB_BYTES,
        "NES base palette must be {NES_PALETTE_RGB_BYTES} bytes, got {} bytes",
        bytes.len()
    );

    let mut palette = [(0u8, 0u8, 0u8); NES_PALETTE_COLOR_COUNT];
    for (entry, rgb) in palette.iter_mut().zip(bytes.chunks_exact(3)) {
        *entry = (rgb[0], rgb[1], rgb[2]);
    }
    Ok(palette)
}

#[inline]
pub fn apply_nes_emphasis(mode: NesPaletteMode, mask: u8, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let emph_bits = mask & 0xE0;
    if emph_bits == 0 {
        return rgb;
    }

    let (red_bit, green_bit) = match mode {
        NesPaletteMode::Pal => (0x40, 0x20),
        NesPaletteMode::Raw | NesPaletteMode::Ntsc | NesPaletteMode::Custom => (0x20, 0x40),
    };

    const ATTEN_NUM: u16 = 192;
    const ATTEN_DEN: u16 = 235;

    let (mut r, mut g, mut b) = rgb;
    if emph_bits & red_bit == 0 {
        r = scale_u8(r, ATTEN_NUM, ATTEN_DEN);
    }
    if emph_bits & green_bit == 0 {
        g = scale_u8(g, ATTEN_NUM, ATTEN_DEN);
    }
    if emph_bits & 0x80 == 0 {
        b = scale_u8(b, ATTEN_NUM, ATTEN_DEN);
    }

    (r, g, b)
}

#[inline]
pub fn apply_rgb_ppu_emphasis(mask: u8, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let emph_bits = mask & 0xE0;
    if emph_bits == 0 {
        return rgb;
    }

    let (mut r, mut g, mut b) = rgb;
    if emph_bits & 0x20 != 0 {
        r = 0xFF;
    }
    if emph_bits & 0x40 != 0 {
        g = 0xFF;
    }
    if emph_bits & 0x80 != 0 {
        b = 0xFF;
    }

    (r, g, b)
}

#[rustfmt::skip]
pub static NES_PALETTE: NesBasePalette = [
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

const fn rgb_ppu_channel(dac: u16) -> u8 {
    ((dac * 255 + 3) / 7) as u8
}

const fn rgb_ppu_color(rgb: u16) -> (u8, u8, u8) {
    (
        rgb_ppu_channel((rgb >> 8) & 0x0F),
        rgb_ppu_channel((rgb >> 4) & 0x0F),
        rgb_ppu_channel(rgb & 0x0F),
    )
}

#[rustfmt::skip]
pub static NES_RGB_2C03_PALETTE: NesBasePalette = [
    rgb_ppu_color(0x333), rgb_ppu_color(0x014), rgb_ppu_color(0x006), rgb_ppu_color(0x326),
    rgb_ppu_color(0x403), rgb_ppu_color(0x503), rgb_ppu_color(0x510), rgb_ppu_color(0x420),
    rgb_ppu_color(0x320), rgb_ppu_color(0x120), rgb_ppu_color(0x031), rgb_ppu_color(0x040),
    rgb_ppu_color(0x022), rgb_ppu_color(0x000), rgb_ppu_color(0x000), rgb_ppu_color(0x000),

    rgb_ppu_color(0x555), rgb_ppu_color(0x036), rgb_ppu_color(0x027), rgb_ppu_color(0x407),
    rgb_ppu_color(0x507), rgb_ppu_color(0x704), rgb_ppu_color(0x700), rgb_ppu_color(0x630),
    rgb_ppu_color(0x430), rgb_ppu_color(0x140), rgb_ppu_color(0x040), rgb_ppu_color(0x053),
    rgb_ppu_color(0x044), rgb_ppu_color(0x000), rgb_ppu_color(0x000), rgb_ppu_color(0x000),

    rgb_ppu_color(0x777), rgb_ppu_color(0x357), rgb_ppu_color(0x447), rgb_ppu_color(0x637),
    rgb_ppu_color(0x707), rgb_ppu_color(0x737), rgb_ppu_color(0x740), rgb_ppu_color(0x750),
    rgb_ppu_color(0x660), rgb_ppu_color(0x360), rgb_ppu_color(0x070), rgb_ppu_color(0x276),
    rgb_ppu_color(0x077), rgb_ppu_color(0x000), rgb_ppu_color(0x000), rgb_ppu_color(0x000),

    rgb_ppu_color(0x777), rgb_ppu_color(0x567), rgb_ppu_color(0x657), rgb_ppu_color(0x757),
    rgb_ppu_color(0x747), rgb_ppu_color(0x755), rgb_ppu_color(0x764), rgb_ppu_color(0x772),
    rgb_ppu_color(0x773), rgb_ppu_color(0x572), rgb_ppu_color(0x473), rgb_ppu_color(0x276),
    rgb_ppu_color(0x467), rgb_ppu_color(0x000), rgb_ppu_color(0x000), rgb_ppu_color(0x000),
];

impl Ppu {
    #[inline]
    pub fn compose_pixel(&mut self) -> u8 {
        let x = self.dot.wrapping_sub(1) as u8;

        let mut bg_pixel: u8 = 0;
        let mut bg_palette: u8 = 0;

        if self.show_bg() && (x >= 8 || self.show_bg_left8()) {
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

        if self.show_sprites() && (x >= 8 || self.show_sprites_left8()) {
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
    use super::{
        NES_RGB_2C03_PALETTE, NesPalette, NesPaletteMode, apply_nes_emphasis,
        apply_nes_palette_mode, apply_rgb_ppu_emphasis, parse_nes_palette_bytes,
    };

    #[test]
    fn raw_mode_is_identity() {
        assert_eq!(
            apply_nes_palette_mode(NesPaletteMode::Raw, (100, 150, 200)),
            (100, 150, 200)
        );
    }

    #[test]
    fn custom_mode_does_not_modify_palette_file_colors() {
        assert_eq!(
            apply_nes_palette_mode(NesPaletteMode::Custom, (100, 150, 200)),
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

    #[test]
    fn parses_64_color_binary_palette() {
        let bytes: Vec<u8> = (0..192u16).map(|v| v as u8).collect();
        let palette = parse_nes_palette_bytes(&bytes).unwrap();
        let base = palette.base();

        assert_eq!(base[0], (0, 1, 2));
        assert_eq!(base[63], (189, 190, 191));
    }

    #[test]
    fn parses_emphasis_palette_preserving_groups() {
        let bytes: Vec<u8> = (0..1536u16).map(|v| v as u8).collect();
        let palette = parse_nes_palette_bytes(&bytes).unwrap();

        let NesPalette::WithEmphasis(groups) = palette else {
            panic!("1536-byte palette should preserve emphasis groups");
        };
        assert_eq!(groups[0][0], (0, 1, 2));
        assert_eq!(groups[0][63], (189, 190, 191));
        assert_eq!(groups[1][0], (192, 193, 194));
    }

    #[test]
    fn rejects_unrecognized_palette_size() {
        let err = parse_nes_palette_bytes(&[0u8; 191]).unwrap_err();
        assert!(err.to_string().contains("192 bytes"));
    }

    #[test]
    fn emphasis_is_identity_when_no_emphasis_bits_are_set() {
        assert_eq!(
            apply_nes_emphasis(NesPaletteMode::Ntsc, 0x00, (180, 120, 90)),
            (180, 120, 90)
        );
    }

    #[test]
    fn pal_emphasis_swaps_red_and_green_bits() {
        let src = (180, 120, 90);

        let ntsc_bit_5 = apply_nes_emphasis(NesPaletteMode::Ntsc, 0x20, src);
        assert_eq!(ntsc_bit_5.0, src.0);
        assert!(ntsc_bit_5.1 < src.1);
        assert!(ntsc_bit_5.2 < src.2);

        let pal_bit_5 = apply_nes_emphasis(NesPaletteMode::Pal, 0x20, src);
        assert!(pal_bit_5.0 < src.0);
        assert_eq!(pal_bit_5.1, src.1);
        assert!(pal_bit_5.2 < src.2);
    }

    #[test]
    fn rgb_2c03_palette_uses_rgb_dac_values() {
        assert_eq!(NES_RGB_2C03_PALETTE[0x21], (109, 182, 255));
        assert_eq!(NES_RGB_2C03_PALETTE[0x29], (109, 219, 0));
        assert_eq!(NES_RGB_2C03_PALETTE[0x30], (255, 255, 255));
        assert_eq!(NES_RGB_2C03_PALETTE[0x0F], (0, 0, 0));
    }

    #[test]
    fn rgb_ppu_emphasis_sets_selected_channels_to_full_scale() {
        assert_eq!(apply_rgb_ppu_emphasis(0x20, (10, 20, 30)), (255, 20, 30));
        assert_eq!(apply_rgb_ppu_emphasis(0xC0, (10, 20, 30)), (10, 255, 255));
    }
}

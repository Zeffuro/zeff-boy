use crate::hardware::ppu::PPU;
use crate::settings::ColorCorrection;

pub(crate) const PALETTE_COLORS: [[u8; 4]; 4] = [
    [224, 248, 208, 255],
    [136, 192, 112, 255],
    [52, 104, 86, 255],
    [8, 24, 32, 255],
];

pub(crate) fn apply_palette(palette: u8, color_id: u8) -> [u8; 4] {
    let shade = (palette >> (color_id * 2)) & 0x03;
    PALETTE_COLORS[shade as usize]
}

fn expand_5bit_to_8bit(value: u8) -> u8 {
    (value << 3) | (value >> 2)
}

fn rgb555_to_rgba(low: u8, high: u8) -> [u8; 4] {
    let packed = u16::from(low) | (u16::from(high) << 8);
    let r = expand_5bit_to_8bit((packed & 0x1F) as u8);
    let g = expand_5bit_to_8bit(((packed >> 5) & 0x1F) as u8);
    let b = expand_5bit_to_8bit(((packed >> 10) & 0x1F) as u8);
    [r, g, b, 255]
}

pub(crate) fn correct_color(
    rgba: [u8; 4],
    correction: ColorCorrection,
    custom_matrix: [f32; 9],
) -> [u8; 4] {
    match correction {
        ColorCorrection::None => rgba,
        ColorCorrection::GbcLcd => {
            let r = rgba[0] as u16;
            let g = rgba[1] as u16;
            let b = rgba[2] as u16;
            let r_out = ((r * 26 + g * 4 + b * 2) / 32).min(255) as u8;
            let g_out = ((g * 24 + b * 8) / 32).min(255) as u8;
            let b_out = ((r * 6 + g * 4 + b * 22) / 32).min(255) as u8;
            [r_out, g_out, b_out, rgba[3]]
        }
        ColorCorrection::Custom => {
            let r = rgba[0] as f32;
            let g = rgba[1] as f32;
            let b = rgba[2] as f32;

            let r_out = (r * custom_matrix[0] + g * custom_matrix[1] + b * custom_matrix[2])
                .clamp(0.0, 255.0) as u8;
            let g_out = (r * custom_matrix[3] + g * custom_matrix[4] + b * custom_matrix[5])
                .clamp(0.0, 255.0) as u8;
            let b_out = (r * custom_matrix[6] + g * custom_matrix[7] + b * custom_matrix[8])
                .clamp(0.0, 255.0) as u8;
            [r_out, g_out, b_out, rgba[3]]
        }
    }
}

pub(crate) fn cgb_palette_rgba(
    palette_ram: &[u8; 64],
    palette_index: u8,
    color_id: u8,
    correction: ColorCorrection,
    custom_matrix: [f32; 9],
) -> [u8; 4] {
    let palette_base = ((palette_index & 0x07) as usize) * 8;
    let color_base = palette_base + ((color_id & 0x03) as usize) * 2;
    let low = palette_ram[color_base];
    let high = palette_ram[color_base + 1];
    correct_color(rgb555_to_rgba(low, high), correction, custom_matrix)
}

impl PPU {
    pub(crate) fn cgb_bg_rgba(&self, palette_index: u8, color_id: u8) -> [u8; 4] {
        cgb_palette_rgba(
            &self.bg_palette_ram,
            palette_index,
            color_id,
            ColorCorrection::None,
            [0.0; 9],
        )
    }

    #[allow(dead_code)]
    pub(crate) fn cgb_obj_rgba(&self, palette_index: u8, color_id: u8) -> [u8; 4] {
        cgb_palette_rgba(
            &self.obj_palette_ram,
            palette_index,
            color_id,
            ColorCorrection::None,
            [0.0; 9],
        )
    }

    pub(crate) fn read_bcps(&self) -> u8 {
        self.bcps | 0x40
    }

    pub(crate) fn write_bcps(&mut self, value: u8) {
        self.bcps = value & 0xBF;
    }

    pub(crate) fn read_bcpd(&self) -> u8 {
        if !self.cpu_palette_accessible() {
            return 0xFF;
        }
        let index = (self.bcps & 0x3F) as usize;
        self.bg_palette_ram[index]
    }

    pub(crate) fn write_bcpd(&mut self, value: u8) {
        if !self.cpu_palette_accessible() {
            return;
        }
        let index = (self.bcps & 0x3F) as usize;
        self.bg_palette_ram[index] = value;
        if self.bcps & 0x80 != 0 {
            self.bcps = (self.bcps & 0x80) | ((index as u8).wrapping_add(1) & 0x3F);
        }
    }

    pub(crate) fn read_ocps(&self) -> u8 {
        self.ocps | 0x40
    }

    pub(crate) fn write_ocps(&mut self, value: u8) {
        self.ocps = value & 0xBF;
    }

    pub(crate) fn read_ocpd(&self) -> u8 {
        if !self.cpu_palette_accessible() {
            return 0xFF;
        }
        let index = (self.ocps & 0x3F) as usize;
        self.obj_palette_ram[index]
    }

    pub(crate) fn write_ocpd(&mut self, value: u8) {
        if !self.cpu_palette_accessible() {
            return;
        }
        let index = (self.ocps & 0x3F) as usize;
        self.obj_palette_ram[index] = value;
        if self.ocps & 0x80 != 0 {
            self.ocps = (self.ocps & 0x80) | ((index as u8).wrapping_add(1) & 0x3F);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb555_decoding_expands_channels() {
        assert_eq!(rgb555_to_rgba(0xFF, 0x7F), [255, 255, 255, 255]);
        assert_eq!(rgb555_to_rgba(0x1F, 0x00), [255, 0, 0, 255]);
    }

    #[test]
    fn bcps_autoincrement_wraps_index() {
        let mut ppu = PPU::new();
        ppu.write_bcps(0xBF);
        ppu.write_bcpd(0x12);
        assert_eq!(ppu.bg_palette_ram[63], 0x12);
        assert_eq!(ppu.bcps, 0x80);
    }

    #[test]
    fn cgb_bg_lookup_uses_palette_and_color_slot() {
        let mut ppu = PPU::new();
        ppu.bg_palette_ram[22] = 0x00;
        ppu.bg_palette_ram[23] = 0x7C;
        assert_eq!(ppu.cgb_bg_rgba(2, 3), [0, 0, 255, 255]);
    }

    #[test]
    fn cgb_obj_lookup_uses_palette_and_color_slot() {
        let mut ppu = PPU::new();
        ppu.obj_palette_ram[42] = 0xE0;
        ppu.obj_palette_ram[43] = 0x03; // green max
        assert_eq!(ppu.cgb_obj_rgba(5, 1), [0, 255, 0, 255]);
    }

    #[test]
    fn bcpd_is_blocked_in_mode3_when_lcd_enabled() {
        let mut ppu = PPU::new();
        ppu.write_bcps(0x80 | 0x02);
        ppu.stat = (ppu.stat & !0x03) | 0x03;
        ppu.bg_palette_ram[2] = 0x55;

        assert_eq!(ppu.read_bcpd(), 0xFF);
        ppu.write_bcpd(0xAA);
        assert_eq!(ppu.bg_palette_ram[2], 0x55);
        assert_eq!(ppu.bcps & 0x3F, 0x02);
    }

    #[test]
    fn ocpd_is_blocked_in_mode3_when_lcd_enabled() {
        let mut ppu = PPU::new();
        ppu.write_ocps(0x80 | 0x01);
        ppu.stat = (ppu.stat & !0x03) | 0x03;
        ppu.obj_palette_ram[1] = 0x66;

        assert_eq!(ppu.read_ocpd(), 0xFF);
        ppu.write_ocpd(0xBB);
        assert_eq!(ppu.obj_palette_ram[1], 0x66);
        assert_eq!(ppu.ocps & 0x3F, 0x01);
    }

    #[test]
    fn bcpd_write_autoincrements_outside_mode3() {
        let mut ppu = PPU::new();
        ppu.write_bcps(0x80 | 0x01);
        ppu.stat = (ppu.stat & !0x03) | 0x00;

        ppu.write_bcpd(0x12);

        assert_eq!(ppu.bg_palette_ram[1], 0x12);
        assert_eq!(ppu.bcps & 0x3F, 0x02);
    }

    #[test]
    fn correct_color_none_is_identity() {
        let rgba = [128, 64, 200, 255];
        assert_eq!(
            correct_color(
                rgba,
                ColorCorrection::None,
                [
                    1.0, 0.0, 0.0, // R
                    0.0, 1.0, 0.0, // G
                    0.0, 0.0, 1.0, // B
                ],
            ),
            rgba
        );
    }

    #[test]
    fn correct_color_gbc_lcd_shifts_colors() {
        let rgba = correct_color(
            [255, 0, 0, 255],
            ColorCorrection::GbcLcd,
            [
                1.0, 0.0, 0.0, // R
                0.0, 1.0, 0.0, // G
                0.0, 0.0, 1.0, // B
            ],
        );
        assert_eq!(rgba[0], 207);
        assert_eq!(rgba[1], 0);
        assert_eq!(rgba[2], 47);
        assert_eq!(rgba[3], 255);
    }

    #[test]
    fn correct_color_gbc_lcd_preserves_alpha() {
        let rgba = correct_color(
            [100, 100, 100, 128],
            ColorCorrection::GbcLcd,
            [
                1.0, 0.0, 0.0, // R
                0.0, 1.0, 0.0, // G
                0.0, 0.0, 1.0, // B
            ],
        );
        assert_eq!(rgba[3], 128);
    }

    #[test]
    fn correct_color_custom_uses_matrix() {
        // Swap R/B channels.
        let matrix = [
            0.0, 0.0, 1.0, // R' = B
            0.0, 1.0, 0.0, // G' = G
            1.0, 0.0, 0.0, // B' = R
        ];
        let rgba = correct_color([200, 50, 10, 255], ColorCorrection::Custom, matrix);
        assert_eq!(rgba, [10, 50, 200, 255]);
    }

    #[test]
    fn cgb_bg_rgba_always_returns_raw_rgb() {
        let mut ppu = PPU::new();
        ppu.bg_palette_ram[0] = 0x1F;
        ppu.bg_palette_ram[1] = 0x00;

        ppu.color_correction = ColorCorrection::None;
        let raw = ppu.cgb_bg_rgba(0, 0);
        assert_eq!(raw, [255, 0, 0, 255]);
        ppu.color_correction = ColorCorrection::GbcLcd;
        let still_raw = ppu.cgb_bg_rgba(0, 0);
        assert_eq!(still_raw, raw);
    }
}

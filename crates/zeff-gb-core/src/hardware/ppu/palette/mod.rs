use crate::hardware::ppu::PPU;
use crate::color_correction::ColorCorrection;

pub const PALETTE_COLORS: [[u8; 4]; 4] = [
    [224, 248, 208, 255],
    [136, 192, 112, 255],
    [52, 104, 86, 255],
    [8, 24, 32, 255],
];

pub fn apply_palette(palette: u8, color_id: u8) -> [u8; 4] {
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

pub fn correct_color(
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

pub fn cgb_palette_rgba(palette_ram: &[u8; 64], palette_index: u8, color_id: u8) -> [u8; 4] {
    let palette_base = ((palette_index & 0x07) as usize) * 8;
    let color_base = palette_base + ((color_id & 0x03) as usize) * 2;
    let low = palette_ram[color_base];
    let high = palette_ram[color_base + 1];
    rgb555_to_rgba(low, high)
}

impl PPU {
    pub fn cgb_bg_rgba(&self, palette_index: u8, color_id: u8) -> [u8; 4] {
        cgb_palette_rgba(&self.bg_palette_ram, palette_index, color_id)
    }

    pub fn cgb_obj_rgba(&self, palette_index: u8, color_id: u8) -> [u8; 4] {
        cgb_palette_rgba(&self.obj_palette_ram, palette_index, color_id)
    }

    pub fn read_bcps(&self) -> u8 {
        self.bcps | 0x40
    }

    pub fn write_bcps(&mut self, value: u8) {
        self.bcps = value & 0xBF;
    }

    pub fn read_bcpd(&self) -> u8 {
        if !self.cpu_palette_accessible() {
            return 0xFF;
        }
        let index = (self.bcps & 0x3F) as usize;
        self.bg_palette_ram[index]
    }

    pub fn write_bcpd(&mut self, value: u8) {
        if !self.cpu_palette_accessible() {
            return;
        }
        let index = (self.bcps & 0x3F) as usize;
        self.bg_palette_ram[index] = value;
        if self.bcps & 0x80 != 0 {
            self.bcps = (self.bcps & 0x80) | ((index as u8).wrapping_add(1) & 0x3F);
        }
    }

    pub fn read_ocps(&self) -> u8 {
        self.ocps | 0x40
    }

    pub fn write_ocps(&mut self, value: u8) {
        self.ocps = value & 0xBF;
    }

    pub fn read_ocpd(&self) -> u8 {
        if !self.cpu_palette_accessible() {
            return 0xFF;
        }
        let index = (self.ocps & 0x3F) as usize;
        self.obj_palette_ram[index]
    }

    pub fn write_ocpd(&mut self, value: u8) {
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
mod tests;

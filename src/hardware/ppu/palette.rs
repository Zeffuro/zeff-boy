use crate::hardware::ppu::PPU;

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

pub(crate) fn cgb_palette_rgba(palette_ram: &[u8; 64], palette_index: u8, color_id: u8) -> [u8; 4] {
    let palette_base = ((palette_index & 0x07) as usize) * 8;
    let color_base = palette_base + ((color_id & 0x03) as usize) * 2;
    let low = palette_ram[color_base];
    let high = palette_ram[color_base + 1];
    rgb555_to_rgba(low, high)
}

impl PPU {
    pub(crate) fn cgb_bg_rgba(&self, palette_index: u8, color_id: u8) -> [u8; 4] {
        cgb_palette_rgba(&self.bg_palette_ram, palette_index, color_id)
    }

    pub(crate) fn cgb_obj_rgba(&self, palette_index: u8, color_id: u8) -> [u8; 4] {
        cgb_palette_rgba(&self.obj_palette_ram, palette_index, color_id)
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
}


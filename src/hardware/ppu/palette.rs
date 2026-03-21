// DMG 4-shade palette colors (RGBA)
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


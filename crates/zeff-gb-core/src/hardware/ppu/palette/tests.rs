use super::*;
use crate::color_correction::ColorCorrection;
use crate::hardware::ppu::PPU;

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

    let raw = ppu.cgb_bg_rgba(0, 0);
    assert_eq!(raw, [255, 0, 0, 255]);
    let still_raw = ppu.cgb_bg_rgba(0, 0);
    assert_eq!(still_raw, raw);
}

#[test]
fn apply_palette_uses_default_dmg_green_preset() {
    assert_eq!(
        apply_palette(0b00_01_10_11, 0),
        apply_dmg_palette(DmgPalettePreset::DmgGreen, 0b00_01_10_11, 0)
    );
}

#[test]
fn dmg_palette_presets_include_required_gray_and_green() {
    assert_eq!(
        apply_dmg_palette(DmgPalettePreset::Gray, 0b00_01_10_11, 0),
        [0, 0, 0, 255]
    );
    assert_eq!(
        apply_dmg_palette(DmgPalettePreset::Gray, 0b00_01_10_11, 3),
        [255, 255, 255, 255]
    );
    assert_eq!(
        apply_dmg_palette(DmgPalettePreset::DmgGreen, 0b00_01_10_11, 3),
        [224, 248, 208, 255]
    );
}

#[test]
fn ppu_exposes_dmg_palette_preset_setter_getter() {
    let mut ppu = PPU::new();
    assert_eq!(ppu.dmg_palette_preset(), DmgPalettePreset::DmgGreen);
    ppu.set_dmg_palette_preset(DmgPalettePreset::Pocket);
    assert_eq!(ppu.dmg_palette_preset(), DmgPalettePreset::Pocket);
}

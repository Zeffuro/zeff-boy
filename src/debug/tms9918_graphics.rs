use crate::debug::common::sega8_palette_rgba;
use crate::debug::types::Sega8GraphicsData;
use zeff_sega8_core::hardware::cartridge::Sega8System;
use zeff_sega8_core::hardware::vdp::{Tms9918Mode, Tms9918VdpDebugSnapshot, tms9918_palette_rgba};

pub(crate) const TMS_TILE_COUNT: usize = 256;
pub(crate) const TMS_TILE_COLUMNS: usize = 16;
pub(crate) const TMS_TILE_SIZE: usize = 8;
pub(crate) const TMS_MAP_ROWS: usize = 24;
pub(crate) const TMS_GRAPHICS_II_SECTION_BYTES: usize = 0x800;

#[derive(Clone, Copy)]
pub(crate) struct TmsMapCell {
    pub(crate) column: usize,
    pub(crate) row: usize,
    pub(crate) tile: usize,
    pub(crate) section: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct TmsSprite {
    pub(crate) x: isize,
    pub(crate) y: isize,
    pub(crate) y_raw: u8,
    pub(crate) pattern: usize,
    pub(crate) color: u8,
    pub(crate) early_clock: bool,
}

pub(crate) fn is_tms(gfx: &Sega8GraphicsData) -> bool {
    !gfx.mode4.enabled
}

pub(crate) fn mode_label(mode: Tms9918Mode) -> &'static str {
    match mode {
        Tms9918Mode::GraphicsI => "Graphics I",
        Tms9918Mode::GraphicsII => "Graphics II",
        Tms9918Mode::Multicolor => "Multicolor",
        Tms9918Mode::Text => "Text",
        Tms9918Mode::Invalid => "Invalid",
    }
}

pub(crate) fn atlas_page_count(tms: &Tms9918VdpDebugSnapshot) -> usize {
    if matches!(tms.mode, Tms9918Mode::GraphicsII) {
        3
    } else {
        1
    }
}

pub(crate) fn pattern_address(
    tms: &Tms9918VdpDebugSnapshot,
    tile: usize,
    section: usize,
    row: usize,
) -> usize {
    let section_offset = if matches!(tms.mode, Tms9918Mode::GraphicsII) {
        section.min(2) * TMS_GRAPHICS_II_SECTION_BYTES
    } else {
        0
    };
    tms.pattern_table_base + section_offset + (tile & 0xFF) * TMS_TILE_SIZE + (row & 7)
}

pub(crate) fn color_address(
    tms: &Tms9918VdpDebugSnapshot,
    tile: usize,
    section: usize,
    row: usize,
) -> Option<usize> {
    match tms.mode {
        Tms9918Mode::GraphicsI => Some(tms.color_table_base + (tile & 0xFF) / 8),
        Tms9918Mode::GraphicsII => Some(
            tms.color_table_base
                + section.min(2) * TMS_GRAPHICS_II_SECTION_BYTES
                + (tile & 0xFF) * TMS_TILE_SIZE
                + (row & 7),
        ),
        _ => None,
    }
}

pub(crate) fn tile_color(
    vram: &[u8],
    tms: &Tms9918VdpDebugSnapshot,
    tile: usize,
    section: usize,
    x: usize,
    y: usize,
) -> u8 {
    match tms.mode {
        Tms9918Mode::GraphicsI | Tms9918Mode::GraphicsII => {
            let pattern = vram_at(vram, pattern_address(tms, tile, section, y));
            let color = color_address(tms, tile, section, y)
                .map(|address| vram_at(vram, address))
                .unwrap_or(0);
            if pattern & (0x80 >> (x & 7)) != 0 {
                color >> 4
            } else {
                color & 0x0F
            }
        }
        Tms9918Mode::Multicolor => {
            let address = tms.pattern_table_base + (tile & 0xFF) * TMS_TILE_SIZE + (y / 4) * 2;
            let color = vram_at(vram, address);
            if x & 7 < 4 { color >> 4 } else { color & 0x0F }
        }
        Tms9918Mode::Text => {
            if x & 7 >= 6 {
                tms.text_background_color
            } else if vram_at(vram, pattern_address(tms, tile, 0, y)) & (0x80 >> (x & 7)) != 0 {
                tms.text_foreground_color
            } else {
                tms.text_background_color
            }
        }
        Tms9918Mode::Invalid => tms.backdrop_color,
    }
}

pub(crate) fn color_rgba(gfx: &Sega8GraphicsData, color: u8) -> [u8; 4] {
    let color = if color & 0x0F == 0 {
        gfx.tms9918.backdrop_color
    } else {
        color & 0x0F
    };
    match gfx.system {
        Sega8System::GameGear => sega8_palette_rgba(gfx.system, &gfx.cram, 16 + usize::from(color)),
        Sega8System::MasterSystem | Sega8System::Sg1000 => tms9918_palette_rgba(color),
    }
}

pub(crate) fn map_cell_at(
    vram: &[u8],
    tms: &Tms9918VdpDebugSnapshot,
    x: usize,
    y: usize,
) -> Option<TmsMapCell> {
    if y >= TMS_MAP_ROWS * TMS_TILE_SIZE {
        return None;
    }
    let (column, row) = match tms.mode {
        Tms9918Mode::Text => {
            if !(8..248).contains(&x) {
                return None;
            }
            ((x - 8) / 6, y / TMS_TILE_SIZE)
        }
        Tms9918Mode::Invalid => return None,
        _ if x < 256 => (x / TMS_TILE_SIZE, y / TMS_TILE_SIZE),
        _ => return None,
    };
    let columns = if matches!(tms.mode, Tms9918Mode::Text) {
        40
    } else {
        32
    };
    let tile = usize::from(vram_at(vram, tms.name_table_base + row * columns + column));
    Some(TmsMapCell {
        column,
        row,
        tile,
        section: row / 8,
    })
}

pub(crate) fn map_color(vram: &[u8], tms: &Tms9918VdpDebugSnapshot, x: usize, y: usize) -> u8 {
    let Some(cell) = map_cell_at(vram, tms, x, y) else {
        return tms.backdrop_color;
    };
    let local_x = if matches!(tms.mode, Tms9918Mode::Text) {
        (x - 8) % 6
    } else {
        x & 7
    };
    tile_color(vram, tms, cell.tile, cell.section, local_x, y & 7)
}

pub(crate) fn sprite_at(gfx: &Sega8GraphicsData, index: usize) -> Option<TmsSprite> {
    sprite_from_vram(&gfx.vram, &gfx.tms9918, index)
}

pub(crate) fn sprite_from_vram(
    vram: &[u8],
    tms: &Tms9918VdpDebugSnapshot,
    index: usize,
) -> Option<TmsSprite> {
    if index >= 32 {
        return None;
    }
    let base = tms.sprite_attribute_table_base + index * 4;
    let y_raw = vram_at(vram, base);
    if y_raw == 0xD0 {
        return None;
    }
    let tag = vram_at(vram, base + 3);
    let early_clock = tag & 0x80 != 0;
    Some(TmsSprite {
        x: isize::from(vram_at(vram, base + 1)) - if early_clock { 32 } else { 0 },
        y: isize::from(y_raw as i8) + 1,
        y_raw,
        pattern: usize::from(vram_at(vram, base + 2)),
        color: tag & 0x0F,
        early_clock,
    })
}

pub(crate) fn sprite_pattern_address(
    tms: &Tms9918VdpDebugSnapshot,
    pattern: usize,
    x: usize,
    y: usize,
) -> usize {
    let base = if tms.sprite_size == 16 {
        pattern & !3
    } else {
        pattern
    };
    let quadrant = if tms.sprite_size == 16 {
        match (x >= 8, y >= 8) {
            (false, false) => 0,
            (false, true) => 1,
            (true, false) => 2,
            (true, true) => 3,
        }
    } else {
        0
    };
    tms.sprite_pattern_table_base + (base + quadrant) * TMS_TILE_SIZE + (y & 7)
}

pub(crate) fn sprite_pattern_pixel(
    vram: &[u8],
    tms: &Tms9918VdpDebugSnapshot,
    pattern: usize,
    x: usize,
    y: usize,
) -> bool {
    vram_at(vram, sprite_pattern_address(tms, pattern, x, y)) & (0x80 >> (x & 7)) != 0
}

fn vram_at(vram: &[u8], address: usize) -> u8 {
    if vram.is_empty() {
        0
    } else {
        vram[address % vram.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(mode: Tms9918Mode) -> Tms9918VdpDebugSnapshot {
        Tms9918VdpDebugSnapshot {
            mode,
            name_table_base: 0x1800,
            pattern_table_base: 0x0800,
            color_table_base: 0x2000,
            sprite_attribute_table_base: 0x1B00,
            sprite_pattern_table_base: 0x0000,
            backdrop_color: 1,
            text_foreground_color: 15,
            text_background_color: 4,
            sprite_size: 8,
            sprite_magnified: false,
        }
    }

    #[test]
    fn graphics_i_uses_group_color_and_pattern_addresses() {
        let tms = snapshot(Tms9918Mode::GraphicsI);
        let mut vram = vec![0; 0x4000];
        vram[pattern_address(&tms, 9, 0, 2)] = 0x80;
        vram[color_address(&tms, 9, 0, 2).unwrap()] = 0xA4;

        assert_eq!(pattern_address(&tms, 9, 0, 2), 0x084A);
        assert_eq!(color_address(&tms, 9, 0, 2), Some(0x2001));
        assert_eq!(tile_color(&vram, &tms, 9, 0, 0, 2), 10);
        assert_eq!(tile_color(&vram, &tms, 9, 0, 1, 2), 4);
    }

    #[test]
    fn graphics_ii_selects_the_matching_section() {
        let tms = snapshot(Tms9918Mode::GraphicsII);

        assert_eq!(pattern_address(&tms, 2, 2, 7), 0x1817);
        assert_eq!(color_address(&tms, 2, 2, 7), Some(0x3017));
    }

    #[test]
    fn text_and_multicolor_pixels_follow_the_active_mode() {
        let mut vram = vec![0; 0x4000];
        let text = snapshot(Tms9918Mode::Text);
        vram[pattern_address(&text, 1, 0, 0)] = 0x80;
        assert_eq!(tile_color(&vram, &text, 1, 0, 0, 0), 15);
        assert_eq!(tile_color(&vram, &text, 1, 0, 6, 0), 4);

        let multicolor = snapshot(Tms9918Mode::Multicolor);
        vram[multicolor.pattern_table_base] = 0xC2;
        assert_eq!(tile_color(&vram, &multicolor, 0, 0, 3, 1), 12);
        assert_eq!(tile_color(&vram, &multicolor, 0, 0, 4, 1), 2);
    }

    #[test]
    fn sprite_address_uses_quadrants_for_16_pixel_patterns() {
        let mut tms = snapshot(Tms9918Mode::GraphicsI);
        tms.sprite_size = 16;

        assert_eq!(sprite_pattern_address(&tms, 7, 9, 10), 0x003A);
    }

    #[test]
    fn sprite_decode_applies_early_clock_and_terminator() {
        let tms = snapshot(Tms9918Mode::GraphicsI);
        let mut vram = vec![0; 0x4000];
        vram[tms.sprite_attribute_table_base..tms.sprite_attribute_table_base + 4]
            .copy_from_slice(&[0xFE, 20, 7, 0x8E]);
        vram[tms.sprite_attribute_table_base + 4] = 0xD0;

        let sprite = sprite_from_vram(&vram, &tms, 0).unwrap();
        assert_eq!(
            (sprite.x, sprite.y, sprite.pattern, sprite.color),
            (-12, -1, 7, 14)
        );
        assert!(sprite.early_clock);
        assert!(sprite_from_vram(&vram, &tms, 1).is_none());
    }
}

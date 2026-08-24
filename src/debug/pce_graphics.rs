use crate::debug::PceVdcGraphicsData;

pub(super) const TILE_SIZE: usize = 8;
pub(super) const TILE_COUNT: usize = 0x800;
const VRAM_WORD_MASK: usize = 0x7FFF;

#[derive(Clone, Copy)]
pub(super) enum ColorMode {
    Full,
    PlanesZeroAndOne,
    PlanesTwoAndThree,
}

pub(super) fn background_dimensions(registers: &[u16; 0x14]) -> (usize, usize) {
    let memory_width = registers[0x09];
    let width = match (memory_width >> 4) & 3 {
        0 => 32,
        1 => 64,
        _ => 128,
    };
    let height = if memory_width & 0x40 == 0 { 32 } else { 64 };
    (width, height)
}

pub(super) fn color_mode(registers: &[u16; 0x14]) -> ColorMode {
    let memory_width = registers[0x09];
    if memory_width & 3 != 3 {
        ColorMode::Full
    } else if memory_width & 0x80 == 0 {
        ColorMode::PlanesZeroAndOne
    } else {
        ColorMode::PlanesTwoAndThree
    }
}

pub(super) fn tile_pixel(vram: &[u16], tile: usize, x: usize, y: usize, mode: ColorMode) -> u8 {
    let bit = 7 - (x & 7);
    let base = (tile & 0x0FFF) << 4;
    let planes_zero_one = vram_word(vram, base + (y & 7));
    let planes_two_three = vram_word(vram, base + 8 + (y & 7));
    let (planes_zero_one, planes_two_three) = match mode {
        ColorMode::Full => (planes_zero_one, planes_two_three),
        ColorMode::PlanesZeroAndOne => (planes_zero_one, 0),
        ColorMode::PlanesTwoAndThree => (0, planes_two_three),
    };
    ((planes_zero_one >> bit) & 1) as u8
        | (((planes_zero_one >> (bit + 8)) & 1) as u8) << 1
        | (((planes_two_three >> bit) & 1) as u8) << 2
        | (((planes_two_three >> (bit + 8)) & 1) as u8) << 3
}

pub(super) fn map_palette_index(gfx: &PceVdcGraphicsData, x: usize, y: usize) -> u8 {
    let (width, height) = background_dimensions(&gfx.registers);
    let tile_x = (x / TILE_SIZE) % width;
    let tile_y = (y / TILE_SIZE) % height;
    let entry = vram_word(&gfx.vram, tile_y * width + tile_x);
    let pen = tile_pixel(
        &gfx.vram,
        usize::from(entry & 0x0FFF),
        x % TILE_SIZE,
        y % TILE_SIZE,
        color_mode(&gfx.registers),
    );
    if pen == 0 {
        0
    } else {
        ((entry >> 8) as u8 & 0xF0) | pen
    }
}

pub(super) fn palette_rgba(
    palette: &[zeff_pce_core::hardware::VceColor; 512],
    index: u8,
) -> [u8; 4] {
    let [red, green, blue] = palette[usize::from(index)].rgb8();
    [red, green, blue, 0xFF]
}

pub(super) fn graphics_signature(
    vdc: &PceVdcGraphicsData,
    palette: &[zeff_pce_core::hardware::VceColor; 512],
) -> u64 {
    let mut hasher = crc32fast::Hasher::new();
    for word in &vdc.vram {
        hasher.update(&word.to_le_bytes());
    }
    for word in &vdc.registers {
        hasher.update(&word.to_le_bytes());
    }
    for color in palette {
        hasher.update(&color.raw().to_le_bytes());
    }
    u64::from(hasher.finalize())
}

fn vram_word(vram: &[u16], address: usize) -> u16 {
    vram.get(address & VRAM_WORD_MASK).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graphics(vram: Vec<u16>, memory_width: u16) -> PceVdcGraphicsData {
        let mut registers = [0; 0x14];
        registers[0x09] = memory_width;
        PceVdcGraphicsData { vram, registers }
    }

    #[test]
    fn decodes_four_pce_bitplanes() {
        let mut vram = vec![0; 0x8000];
        vram[0] = 0x8080;
        vram[8] = 0x8080;

        assert_eq!(tile_pixel(&vram, 0, 0, 0, ColorMode::Full), 0x0F);
        assert_eq!(tile_pixel(&vram, 0, 1, 0, ColorMode::Full), 0);
    }

    #[test]
    fn map_uses_palette_and_transparent_zero_pen() {
        let mut vram = vec![0; 0x8000];
        vram[0] = 0xA001;
        vram[16] = 0x0080;

        let gfx = graphics(vram, 0);
        assert_eq!(map_palette_index(&gfx, 0, 0), 0xA1);
        assert_eq!(map_palette_index(&gfx, 1, 0), 0);
    }

    #[test]
    fn memory_width_selects_background_dimensions() {
        let vram = vec![0; 0x8000];
        assert_eq!(
            background_dimensions(&graphics(vram.clone(), 0).registers),
            (32, 32)
        );
        assert_eq!(
            background_dimensions(&graphics(vram.clone(), 0x10).registers),
            (64, 32)
        );
        assert_eq!(
            background_dimensions(&graphics(vram, 0x60).registers),
            (128, 64)
        );
    }

    #[test]
    fn palette_uses_the_vce_background_entry() {
        let mut palette = [zeff_pce_core::hardware::VceColor::new(0); 512];
        palette[0xA1] = zeff_pce_core::hardware::VceColor::new(0x01C0);

        assert_eq!(palette_rgba(&palette, 0xA1), [0, 255, 0, 255]);
    }
}

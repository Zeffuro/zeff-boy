use super::*;

pub(in crate::hardware::vdp) fn tile_height(interlaced: bool) -> usize {
    if interlaced {
        usize::from(INTERLACE_TILE_HEIGHT)
    } else {
        usize::from(TILE_HEIGHT)
    }
}

pub(in crate::hardware::vdp) fn vertical_mask(interlaced: bool) -> u16 {
    if interlaced {
        VSRAM_DATA_MASK
    } else {
        SCROLL_OFFSET_MASK
    }
}

pub(in crate::hardware::vdp) fn pattern_index_mask(interlaced: bool) -> u16 {
    if interlaced {
        INTERLACE_TILE_INDEX_MASK
    } else {
        TILE_INDEX_MASK
    }
}

pub(super) fn pattern_address(name: u16, row: usize, column: usize, interlaced: bool) -> usize {
    let height = tile_height(interlaced);
    let row = if name & TILE_VFLIP != 0 {
        height - 1 - row
    } else {
        row
    };
    usize::from(name & pattern_index_mask(interlaced)) * height * TILE_ROW_BYTES
        + row * TILE_ROW_BYTES
        + column / 2
}

pub(in crate::hardware::vdp) fn sprite_y(value: u16, interlaced: bool) -> i16 {
    if interlaced {
        (value & SPRITE_CACHED_POSITION_MASK) as i16 - INTERLACE_SPRITE_ORIGIN
    } else {
        (value & SPRITE_POSITION_MASK) as i16 - SPRITE_ORIGIN
    }
}

impl VdpMemory {
    pub(super) fn tile_row_in_geometry(
        &self,
        name: u16,
        row: u8,
        interlaced: bool,
    ) -> Result<VdpTileRow, VdpRenderError> {
        if usize::from(row) >= tile_height(interlaced) {
            return Err(VdpRenderError::InvalidRow(row));
        }
        let address = pattern_address(name, usize::from(row), 0, interlaced);
        let mut colors = [0; TILE_WIDTH];
        for (pair, byte) in self.vram()[address..address + TILE_ROW_BYTES]
            .iter()
            .enumerate()
        {
            colors[pair * 2] = byte >> PIXEL_BITS;
            colors[pair * 2 + 1] = byte & PIXEL_COLOR_MASK;
        }
        if name & TILE_HFLIP != 0 {
            colors.reverse();
        }
        Ok(VdpTileRow {
            colors,
            palette: ((name & TILE_PALETTE_MASK) >> TILE_PALETTE_SHIFT) as u8,
            priority: name & TILE_PRIORITY != 0,
        })
    }

    pub(super) fn sample_name(
        &self,
        base: u16,
        width: usize,
        x: usize,
        y: usize,
        interlaced: bool,
    ) -> Result<VdpPixel, VdpRenderError> {
        let cell = (y / tile_height(interlaced)) * width + x / TILE_WIDTH;
        let address = (usize::from(base) + cell * NAME_ENTRY_BYTES) % VRAM_SIZE;
        let vram = self.vram();
        let name = u16::from_be_bytes([vram[address], vram[address + 1]]);
        let column = if name & TILE_HFLIP != 0 {
            TILE_WIDTH - 1 - x % TILE_WIDTH
        } else {
            x % TILE_WIDTH
        };
        let address = pattern_address(name, y % tile_height(interlaced), column, interlaced);
        let shift = if column & 1 == 0 { PIXEL_BITS } else { 0 };
        Ok(VdpPixel {
            color: (vram[address] >> shift) & PIXEL_COLOR_MASK,
            palette: ((name & TILE_PALETTE_MASK) >> TILE_PALETTE_SHIFT) as u8,
            priority: name & TILE_PRIORITY != 0,
        })
    }
}

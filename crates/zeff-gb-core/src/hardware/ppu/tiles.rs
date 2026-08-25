pub fn tile_data_address(tile_index: u8, tile_data_unsigned: bool) -> usize {
    if tile_data_unsigned {
        (tile_index as usize) * 16
    } else {
        ((tile_index as i8 as i16) * 16 + 0x1000) as usize
    }
}

pub fn decode_tile_pixel(vram: &[u8], tile_data_addr: usize, line: usize, pixel: usize) -> u8 {
    decode_tile_row_pixel(read_tile_row(vram, tile_data_addr, line), pixel)
}

pub(super) fn read_tile_row(vram: &[u8], tile_data_addr: usize, line: usize) -> [u8; 2] {
    let lo = vram.get(tile_data_addr + line * 2).copied().unwrap_or(0);
    let hi = vram
        .get(tile_data_addr + line * 2 + 1)
        .copied()
        .unwrap_or(0);

    [lo, hi]
}

pub(super) fn decode_tile_row_pixel(row: [u8; 2], pixel: usize) -> u8 {
    let bit = 7 - pixel as u8;
    ((row[1] >> bit) & 1) << 1 | ((row[0] >> bit) & 1)
}

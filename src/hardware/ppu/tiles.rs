pub(crate) fn tile_data_address(tile_index: u8, tile_data_unsigned: bool) -> usize {
    if tile_data_unsigned {
        (tile_index as usize) * 16
    } else {
        ((tile_index as i8 as i16) * 16 + 0x1000) as usize
    }
}

pub(crate) fn decode_tile_pixel(vram: &[u8], tile_data_addr: usize, line: usize, pixel: usize) -> u8 {
    let lo = vram.get(tile_data_addr + line * 2).copied().unwrap_or(0);
    let hi = vram.get(tile_data_addr + line * 2 + 1).copied().unwrap_or(0);
    let bit = 7 - pixel as u8;
    ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1)
}

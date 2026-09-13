use super::gax::{half, integer};

pub fn fixture() -> Vec<u8> {
    const BASE: usize = 0x40;
    let mut bytes = vec![0; 0x120];
    half(&mut bytes, BASE, 0x0121);
    bytes[BASE + 2] = 1;
    bytes[BASE + 3] = 1;
    let instrument = BASE + 8;
    integer(&mut bytes, instrument, 4);
    integer(&mut bytes, instrument + 8, 4);
    bytes[instrument + 12] = 64;
    bytes[instrument + 13] = 128;
    for envelope in [instrument + 20, instrument + 72] {
        bytes[envelope + 1..envelope + 4].fill(255);
    }
    bytes[instrument + 124..instrument + 128].copy_from_slice(&[0x80, 0, 0x7f, 0xff]);
    let song = instrument + 128;
    integer(&mut bytes, BASE + 4, (song - BASE) as u32);
    bytes[song..song + 6].copy_from_slice(&[2, 2, 0, 1, 6, 125]);
    let pattern = song + 12;
    half(&mut bytes, pattern, 2);
    let row_data = pattern + 12;
    integer(&mut bytes, pattern + 8, (row_data - BASE) as u32);
    bytes[row_data..row_data + 4].copy_from_slice(&[0xc0, 0, 49, 1]);
    bytes
}

pub fn unreachable_zero_pattern() -> Vec<u8> {
    let mut bytes = fixture();
    bytes[0xc8..0xce].copy_from_slice(&[2, 3, 2, 3, 6, 125]);
    bytes[0xd0..0xe8].fill(0);
    bytes[0xd0..0xd3].copy_from_slice(&[0, 1, 2]);
    half(&mut bytes, 0xd4, 1);
    integer(&mut bytes, 0xd8, 0xf0 - 0x40);
    half(&mut bytes, 0xe0, 1);
    integer(&mut bytes, 0xe4, 0xf8 - 0x40);
    bytes[0xf0..0xf6].copy_from_slice(&[0xd8, 0, 49, 1, 0x0b, 2]);
    bytes[0xf8..0xfc].copy_from_slice(&[0x18, 0, 0x0b, 0]);
    bytes
}

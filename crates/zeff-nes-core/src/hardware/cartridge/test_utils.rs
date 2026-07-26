use super::header::INES_MAGIC;

pub(crate) fn make_header(
    prg_banks: u8,
    chr_banks: u8,
    flags6: u8,
    flags7: u8,
    rest: [u8; 8],
) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(INES_MAGIC);
    h[4] = prg_banks;
    h[5] = chr_banks;
    h[6] = flags6;
    h[7] = flags7;
    h[8..16].copy_from_slice(&rest);
    h
}

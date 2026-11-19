pub(super) use crate::vdp_line::support as base;
pub use base::{REGION, ROW_AT, color, register, tick, words};
use zeff_md_core::Vdp;

pub fn fixture() -> Vdp {
    let mut device = base::fixture();
    words(&mut device, 160, 1, &[0x1234, 0x5678]);
    words(&mut device, 188, 1, &[0x8765, 0x4321]);
    words(&mut device, 0xc000, 1, &[0x8005, 0x0805, 0]);
    words(&mut device, 0xa000, 1, &[0xa002, 0, 0xa002]);
    words(&mut device, 0xc6c0, 1, &[0x8005, 0x0805, 0x9005]);
    words(&mut device, 0xa6c0, 1, &[0xa002, 0, 0xa002]);
    base::sat(&mut device, 0x2000, 1, [192, 2, 0xe004, 152]);
    base::sat(&mut device, 0x2000, 2, [128, 3, 0x4003, 160]);
    base::sat(&mut device, 0x2000, 3, [344, 0, 0xe005, 168]);
    device
}

pub fn expected(row: u16, x: usize) -> u16 {
    let index = match (row, x) {
        (0 | 216, 0..=7) | (7 | 223, 8..=15) => 1 + (x % 8) as u16,
        (7 | 223, 0..=7) | (0 | 216, 8..=15) => 8 - (x % 8) as u16,
        (0, 16..=23) => 18,
        (0, 32..=39) if x.is_multiple_of(2) => 35,
        (base::ROW, _) => return base::expected(x),
        (71, 0..=15) => [1, 15, 2, 15, 3, 15, 4, 15, 15, 4, 15, 3, 15, 2, 15, 1][x],
        (71 | 223, 16..=23) => 1 + (x - 16) as u16,
        (71, 24..=31) => [1, 15, 2, 15, 3, 15, 4, 15][x - 24],
        (216, 16..=23) => 8 - (x - 16) as u16,
        (216, 40..=47) => 49 + (x - 40) as u16,
        (223, 40..=47) => 56 - (x - 40) as u16,
        _ => 15,
    };
    color(index)
}

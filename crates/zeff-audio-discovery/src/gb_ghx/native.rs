use std::sync::atomic::AtomicBool;

use super::{GbGhxSong, PreparedGbGhx, checked_driver};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbGhxSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbGhx> {
    let driver = checked_driver(bytes, song, cancel)?;
    let mut output = bytes.to_vec();
    let mut code = vec![0xf3, 0x31, 0xf0, 0xdf, 0xaf, 0xe0, 0xff, 0xe0, 0x0f];
    code.extend([0x3e, 1, 0xe0, 0x4d, 0x10, 0]);
    code.extend([0x3e, (song.bank >> 8) as u8, 0xea, 0, 0x30]);
    code.extend([0x3e, song.bank as u8, 0xea, 0, 0x20]);
    let [lo, hi] = driver.wram.to_le_bytes();
    // The clear finishes before the first return address is pushed onto the stack.
    code.extend([
        0x21, lo, hi, 0x01, 0, 2, 0xaf, 0x22, 0x0b, 0x78, 0xb1, 0x20, 0xf9,
    ]);
    code.extend([0xcd, 6, 0x40]);
    code.extend([0xaf, 0xe0, 0x80, 0x3e, 0xa5, 0xe0, 0x81]);
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0x80, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    code.extend([0x3e, song.subsong, 0x0e, song.module, 0xcd, 0, 0x40]);
    code.extend([
        0xaf, 0xe0, 0x0f, 0x3e, 1, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd,
    ]);
    anyhow::ensure!(
        code.len() < 0xb0,
        "native startup exceeds its reserved space"
    );
    output[0x150..0x150 + code.len()].copy_from_slice(&code);
    output[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    output[0x200..0x20c].copy_from_slice(&[
        0xf5, 0xc5, 0xd5, 0xe5, 0xcd, 3, 0x40, 0xe1, 0xd1, 0xc1, 0xf1, 0xd9,
    ]);
    output[0x40..0x43].copy_from_slice(&[0xc3, 0, 2]);
    Ok(PreparedGbGhx {
        bytes: output,
        hardware: song.hardware,
        ready_address: 0xff81,
        ready_value: 0xa5,
        ack_address: 0xff80,
        ack_value: 0x5a,
        wait_start,
        wait_end,
    })
}

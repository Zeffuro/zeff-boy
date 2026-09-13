use anyhow::{Result, ensure};

use crate::nes_native::{NesNativeTiming, PreparedNesNative};

use super::NesToseSong;

pub(super) fn build(bytes: &[u8], song: &NesToseSong) -> Result<PreparedNesNative> {
    let mut code = vec![0x78, 0xd8, 0xa2, 0xff, 0x9a, 0xa9, 0, 0xa2, 0];
    for page in 0..8 {
        code.extend([0x9d, 0, page]);
    }
    code.extend([0xe8, 0xd0, 0xe5, 0x8d, 0, 0x20, 0x8d, 1, 0x20]);
    code.extend([
        0x8d, 8, 0x80, 0x8d, 0x0a, 0x80, 0xa9, 0x40, 0x8d, 0x17, 0x40,
    ]);
    code.extend([0x20, 0x6a, 0xca, 0xa9, 1, 0x8d, 0xf0, 7]);
    let wait_start = 0xf800 + code.len() as u16;
    code.extend([0xad, 0xf1, 7, 0xc9, 1, 0xd0, 0xf9]);
    for channel in 0..4 {
        code.extend([0xa0, song.index as u8 + channel, 0x20, 0xfb, 0x84]);
    }
    code.extend([0x2c, 0x02, 0x20, 0xa9, 0x80, 0x8d, 0, 0x20]);
    let idle = 0xf800 + code.len() as u16;
    code.push(0x4c);
    code.extend(idle.to_le_bytes());
    let nmi = 0xf800 + code.len() as u16;
    code.extend([
        0x48, 0x8a, 0x48, 0x98, 0x48, 0x20, 7, 0x80, 0x68, 0xa8, 0x68, 0xaa, 0x68, 0x40,
    ]);
    ensure!(
        code.len() < 256,
        "NES TOSE bootstrap exceeds its isolated window"
    );
    let mut result = bytes.to_vec();
    result[0x3f810..0x3f810 + code.len()].copy_from_slice(&code);
    result[0x4000a..0x4000c].copy_from_slice(&nmi.to_le_bytes());
    result[0x4000c..0x4000e].copy_from_slice(&0xf800_u16.to_le_bytes());
    result[0x4000e..0x40010].copy_from_slice(&(nmi + 13).to_le_bytes());
    Ok(PreparedNesNative {
        bytes: result,
        mapper: 16,
        timing: NesNativeTiming::Ntsc,
        ready_address: 0x7f0,
        ack_address: 0x7f1,
        wait_start,
        wait_end: wait_start + 7,
    })
}

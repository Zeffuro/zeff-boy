use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};

use crate::nes_native::{NesNativeTiming, PreparedNesNative};

use super::NesToseSong;

pub fn prepare_rom(
    bytes: &[u8],
    song: &NesToseSong,
    cancel: &AtomicBool,
) -> Result<PreparedNesNative> {
    super::validate_song(bytes, song, cancel)?;
    if song.profile == super::fcg::PROFILE {
        return super::fcg_native::build(bytes, song);
    }
    if let Some(profile) = super::closed::profile(song.profile) {
        return super::closed_native::build(bytes, song, profile);
    }
    let mut code = vec![
        0x78, 0xd8, 0xa9, 0, 0x8d, 0x03, 0xf8, 0xa2, 0xff, 0x9a, 0xa9, 0, 0xa2, 0,
    ];
    for page in 0..8 {
        code.extend([0x9d, 0, page]);
    }
    code.extend([0xe8, 0xd0, 0xe5, 0x8d, 0, 0x20, 0x8d, 1, 0x20]);
    code.extend([0xa9, 0x40, 0x8d, 0x17, 0x40, 0x20, 0xc0, 0x98]);
    // The immediate operand also supplies the GxROM bus-conflict value.
    let operand = 0xf800 + code.len() as u16 + 1;
    code.extend([0xa9, 0x30, 0x8d]);
    code.extend(operand.to_le_bytes());
    code.extend([0xa9, 1, 0x8d, 0xf0, 7]);
    let wait_start = 0xf800 + code.len() as u16;
    code.extend([0xad, 0xf1, 7, 0xc9, 1, 0xd0, 0xf9]);
    for channel in 0..song.tracks.len() {
        code.extend([0xa9, song.index as u8 + channel as u8, 0x20, 0xa8, 0x98]);
    }
    code.extend([0x2c, 0x02, 0x20, 0xa9, 0x80, 0x8d, 0, 0x20]);
    let idle = 0xf800 + code.len() as u16;
    code.push(0x4c);
    code.extend(idle.to_le_bytes());
    let nmi = 0xf800 + code.len() as u16;
    code.extend([
        0x48, 0x8a, 0x48, 0x98, 0x48, 0x20, 0x68, 0x8f, 0x68, 0xa8, 0x68, 0xaa, 0x68, 0x40,
    ]);
    ensure!(
        code.len() < 256,
        "NES TOSE bootstrap exceeds its isolated window"
    );
    let mut result = bytes.to_vec();
    for bank in 0..4 {
        let base = 16 + bank * 0x8000;
        result[base + 0x7800..base + 0x7800 + code.len()].copy_from_slice(&code);
        result[base + 0x7ffa..base + 0x7ffc].copy_from_slice(&nmi.to_le_bytes());
        result[base + 0x7ffc..base + 0x7ffe].copy_from_slice(&0xf800_u16.to_le_bytes());
        result[base + 0x7ffe..base + 0x8000].copy_from_slice(&(nmi + 13).to_le_bytes());
    }
    Ok(PreparedNesNative {
        bytes: result,
        mapper: 66,
        timing: NesNativeTiming::Ntsc,
        ready_address: 0x7f0,
        ack_address: 0x7f1,
        wait_start,
        wait_end: wait_start + 7,
    })
}

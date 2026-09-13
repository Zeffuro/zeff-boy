use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};

use super::{NesQueue, NesSong};
use crate::nes_native::{NesNativeTiming, PreparedNesNative};

#[cfg(any(test, feature = "test-support"))]
mod fixture;
#[cfg(test)]
mod tests;
#[cfg(any(test, feature = "test-support"))]
pub use fixture::fixture_rom;

pub fn supports_native(song: &NesSong) -> bool {
    song.profile == super::PROFILE
        && song.index < 16
        && !matches!(song.index, 7 | 15)
        && song.channels.iter().any(|channel| channel.note_count != 0)
}

pub fn prepare_rom(bytes: &[u8], song: &NesSong, cancel: &AtomicBool) -> Result<PreparedNesNative> {
    ensure!(
        supports_native(song),
        "NES selector has no qualified native playback"
    );
    super::validate_song(bytes, song, cancel)?;
    build(bytes, song)
}

fn build(bytes: &[u8], song: &NesSong) -> Result<PreparedNesNative> {
    let queue = match song.queue {
        NesQueue::Event => 0xfc,
        NesQueue::Area => 0xfb,
    };
    let mut code = vec![0x78, 0xd8, 0xa2, 0xff, 0x9a, 0xa9, 0, 0xa2, 0];
    for page in 0..8 {
        code.extend([0x9d, 0, page]);
    }
    // Start from cleared driver RAM and disabled rendering.
    code.extend([0xe8, 0xd0, 0xe5, 0x8d, 0, 0x20, 0x8d, 1, 0x20]);
    code.extend([0xa9, 1, 0x8d, 0x70, 7, 0x8d, 0xf0, 7]);
    let wait_start = 0x8000 + code.len() as u16;
    code.extend([0xad, 0xf1, 7, 0xc9, 1, 0xd0, 0xf9]);
    code.extend([0xa9, song.selector, 0x85, queue, 0xa9, 0x80, 0x8d, 0, 0x20]);
    let idle = 0x8000 + code.len() as u16;
    code.push(0x4c);
    code.extend(idle.to_le_bytes());
    let nmi = 0x8000 + code.len() as u16;
    code.extend([
        0x48, 0x8a, 0x48, 0x98, 0x48, 0x20, 0xd0, 0xf2, 0x68, 0xa8, 0x68, 0xaa, 0x68, 0x40,
    ]);
    ensure!(
        code.len() < 0x100,
        "NES bootstrap exceeds its isolated window"
    );
    let mut result = bytes.to_vec();
    result[16..16 + code.len()].copy_from_slice(&code);
    result[0x800a..0x800c].copy_from_slice(&nmi.to_le_bytes());
    result[0x800c..0x800e].copy_from_slice(&0x8000_u16.to_le_bytes());
    result[0x800e..0x8010].copy_from_slice(&(nmi + 13).to_le_bytes());
    Ok(PreparedNesNative {
        bytes: result,
        mapper: 0,
        timing: NesNativeTiming::Ntsc,
        ready_address: 0x7f0,
        ack_address: 0x7f1,
        wait_start,
        wait_end: wait_start + 7,
    })
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn fixture_matches(bytes: &[u8]) -> bool {
    static HASH: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    bytes.len() == 40_976
        && zeff_firmware::sha256_bytes(bytes)
            == *HASH.get_or_init(|| zeff_firmware::sha256_bytes(&fixture_rom()))
}

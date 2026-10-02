use std::sync::atomic::AtomicBool;

use crate::gb_music::native::{GbBankedTiming, PreparedGbBanked};

use super::{GbTimerSong, profiles, validate_song};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbTimerSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbBanked> {
    validate_song(bytes, song, cancel)?;
    let profile = profiles::named(song.profile)
        .ok_or_else(|| anyhow::anyhow!("Timer native profile is not recognized"))?;
    let mut output = bytes.to_vec();
    let mut code = vec![
        0xf3, 0x31, 0, 0xcf, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x07,
    ];
    if bytes[0x147] == 3 {
        code.extend([0xea, 0, 0x60, 0xea, 0, 0x40]);
    }
    // Address 2100 selects a ROM bank on both qualified MBC1 and MBC2 builds.
    code.extend([0x3e, 1, 0xea, 0, 0x21, 0xe0, profile.shadow]);
    code.extend([0xaf, 0xe0, 0xfd, 0x3e, 0xa5, 0xe0, 0xfc]);
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0xfd, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    for address in [profile.init, profile.stop] {
        let target = address.to_le_bytes();
        code.extend([0xcd, target[0], target[1]]);
    }
    let select = profile.select.to_le_bytes();
    code.extend([0x3e, song.index as u8, 0xcd, select[0], select[1]]);
    code.extend([
        0xaf, 0xe0, 0x0f, 0x3e, 4, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd,
    ]);
    anyhow::ensure!(code.len() < 0xb0, "Timer bootstrap exceeds reserved space");
    output[0x150..0x150 + code.len()].copy_from_slice(&code);
    output[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    let tick = profile.tick.to_le_bytes();
    output[0x200..0x20c].copy_from_slice(&[
        0xf5, 0xc5, 0xd5, 0xe5, 0xcd, tick[0], tick[1], 0xe1, 0xd1, 0xc1, 0xf1, 0xd9,
    ]);
    output[0x50..0x53].copy_from_slice(&[0xc3, 0, 2]);
    Ok(PreparedGbBanked {
        bytes: output,
        timing: GbBankedTiming::Dmg,
        ready_address: 0xfffc,
        ready_value: 0xa5,
        ack_address: 0xfffd,
        ack_value: 0x5a,
        wait_start,
        wait_end,
    })
}

use std::sync::atomic::AtomicBool;

use crate::gb_music::native::{GbBankedTiming, PreparedGbBanked};

use super::{GbCosmigoSong, profiles, validate_song};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbCosmigoSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbBanked> {
    validate_song(bytes, song, cancel)?;
    let profile = profiles::named(song.profile)
        .ok_or_else(|| anyhow::anyhow!("Cosmigo native profile is not recognized"))?;
    let mut output = vec![0; 0x8000];
    output[0x143] = 0xc0;
    output[0x147] = 0x19;
    let source = usize::from(song.bank) * 0x4000;
    output[0x4000..].copy_from_slice(&bytes[source..source + 0x4000]);
    output[0x14d] = output[0x134..=0x14c].iter().fold(0_u8, |value, &byte| {
        value.wrapping_sub(byte).wrapping_sub(1)
    });
    let mut code = vec![
        0xf3, 0x31, 0, 0xcf, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0x3e, 1, 0xe0, 0x4d, 0x10, 0, 0x3e, 1,
        0xea, 0, 0x20, 0xe0, 0x70, 0x21, 0, 0xdf, 0x06, 0, 0xaf, 0x22, 0x05, 0x20, 0xfc, 0xaf,
        0xe0, 0x80, 0x3e, 0xa5, 0xe0, 0x81,
    ];
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0x80, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    if profile.separate_start {
        code.extend([0xcd, 0, 0x40, 0x3e, song.index as u8, 0xcd, 0x56, 0x40]);
    } else {
        // This entry writes NR50 before powering the APU.
        code.extend([0x3e, 0x80, 0xe0, 0x26]);
        code.extend([0x3e, song.index as u8, 0xcd, 0, 0x40]);
    }
    code.extend([
        0xaf, 0xe0, 0x0f, 0x3e, 1, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd,
    ]);
    anyhow::ensure!(
        code.len() < 0xb0,
        "Cosmigo bootstrap exceeds its reserved space"
    );
    output[0x150..0x150 + code.len()].copy_from_slice(&code);
    output[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    output[0x200..0x20c].copy_from_slice(&[
        0xf5, 0xc5, 0xd5, 0xe5, 0xcd, 3, 0x40, 0xe1, 0xd1, 0xc1, 0xf1, 0xd9,
    ]);
    output[0x40..0x43].copy_from_slice(&[0xc3, 0, 2]);
    Ok(PreparedGbBanked {
        bytes: output,
        timing: GbBankedTiming::CgbDouble,
        ready_address: 0xff81,
        ready_value: 0xa5,
        ack_address: 0xff80,
        ack_value: 0x5a,
        wait_start,
        wait_end,
    })
}

use std::sync::atomic::AtomicBool;

use crate::gb_music::native::{GbBankedTiming, PreparedGbBanked};

use super::{GbImedSong, profiles, validate_song};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbImedSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbBanked> {
    validate_song(bytes, song, cancel)?;
    let p = profiles::named(song.profile)
        .ok_or_else(|| anyhow::anyhow!("IMEDGBoy native profile is not recognized"))?;
    // Mirrored overdumps were checked before reducing the playback image.
    let mut output = bytes[..0x8000usize << bytes[0x148]].to_vec();
    let mut code = vec![0xf3, 0x31, 0, 0xcf, 0xaf, 0xe0, 0xff, 0xe0, 0x0f];
    if p.double_speed {
        code.extend([0x3e, 1, 0xe0, 0x4d, 0x10, 0, 0xaf, 0xea, 0, 0x30]);
    } else {
        code.extend([0xaf, 0xea, 0, 0x60, 0xea, 0, 0x40]);
    }
    code.extend([
        0x3e,
        song.bank as u8,
        0xea,
        0,
        0x20,
        0x3e,
        p.wram_bank,
        0xe0,
        0x70,
    ]);
    let start = p.state_start.to_le_bytes();
    let flag = p.sfx_flag.to_le_bytes();
    code.extend([
        0x21,
        start[0],
        start[1],
        0x06,
        (p.state_end - p.state_start) as u8,
        0xaf,
        0x22,
        0x05,
        0x20,
        0xfc,
        0xea,
        flag[0],
        flag[1],
    ]);
    if let Some(address) = p.initial_mask {
        let address = address.to_le_bytes();
        code.extend([0x3e, 0xff, 0xea, address[0], address[1], 0xaf]);
    }
    code.extend([0xe0, 0xfd, 0x3e, 0xa5, 0xe0, 0xfc]);
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0xfd, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    let module = song.module_address.to_le_bytes();
    let init = p.init.to_le_bytes();
    code.extend([0x21, module[0], module[1], 0xcd, init[0], init[1]]);
    code.extend([
        0xaf, 0xe0, 0x0f, 0x3e, 1, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd,
    ]);
    anyhow::ensure!(
        code.len() < 0xb0,
        "IMEDGBoy bootstrap exceeds its reserved space"
    );
    output[0x150..0x150 + code.len()].copy_from_slice(&code);
    output[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    let tick = p.tick.to_le_bytes();
    output[0x200..0x211].copy_from_slice(&[
        0xf5,
        0xc5,
        0xd5,
        0xe5,
        0x3e,
        song.bank as u8,
        0xea,
        0,
        0x20,
        0xcd,
        tick[0],
        tick[1],
        0xe1,
        0xd1,
        0xc1,
        0xf1,
        0xd9,
    ]);
    output[0x40..0x43].copy_from_slice(&[0xc3, 0, 2]);
    Ok(PreparedGbBanked {
        bytes: output,
        timing: if p.double_speed {
            GbBankedTiming::CgbDouble
        } else {
            GbBankedTiming::Dmg
        },
        ready_address: 0xfffc,
        ready_value: 0xa5,
        ack_address: 0xfffd,
        ack_value: 0x5a,
        wait_start,
        wait_end,
    })
}

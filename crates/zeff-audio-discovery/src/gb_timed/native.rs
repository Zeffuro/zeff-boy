use std::sync::atomic::AtomicBool;

use crate::gb_music::native::{GbBankedTiming, PreparedGbBanked};

use super::{GbTimedSong, profiles, validate_song};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbTimedSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbBanked> {
    validate_song(bytes, song, cancel)?;
    let profile = profiles::named(song.profile)
        .ok_or_else(|| anyhow::anyhow!("Timed native profile is not recognized"))?;
    for mapped in &song.mapped_spans {
        let start = mapped.effective_offset;
        let end = start + mapped.byte_len;
        for (reserved_start, reserved_end) in [(0x40, 0x43), (0x100, 0x103), (0x150, 0x220)] {
            anyhow::ensure!(
                end <= reserved_start || start >= reserved_end,
                "Timed native wrapper overlaps admitted source bytes"
            );
        }
    }
    let mut output = bytes.to_vec();
    let shadow = profile.shadow.to_le_bytes();
    let mut code = vec![
        0xf3, 0x31, 0, 0xcf, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x07, 0xea, 0, 0x30, 0x3e, 1,
        0xea, 0, 0x20, 0xea, shadow[0], shadow[1],
    ];
    let init = profile.init.to_le_bytes();
    let stop = profile.stop.to_le_bytes();
    code.extend([0xcd, init[0], init[1], 0xcd, stop[0], stop[1]]);
    code.extend([0xaf, 0xe0, 0xfd, 0x3e, 0xa5, 0xe0, 0xfc]);
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0xfd, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    let start = profile.start.to_le_bytes();
    let pointer = song.header_address.to_le_bytes();
    code.extend([
        0x3e,
        song.bank as u8,
        0x11,
        pointer[0],
        pointer[1],
        0xcd,
        start[0],
        start[1],
        0xaf,
        0xe0,
        0x0f,
        0x3e,
        1,
        0xe0,
        0xff,
        0xfb,
        0x76,
        0x18,
        0xfd,
    ]);
    anyhow::ensure!(code.len() < 0xb0, "Timed bootstrap exceeds reserved space");
    output[0x150..0x150 + code.len()].copy_from_slice(&code);
    output[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    let tick = profile.tick.to_le_bytes();
    let second = profile.second_tick.to_le_bytes();
    output[0x200..0x20f].copy_from_slice(&[
        0xf5, 0xc5, 0xd5, 0xe5, 0xcd, tick[0], tick[1], 0xcd, second[0], second[1], 0xe1, 0xd1,
        0xc1, 0xf1, 0xd9,
    ]);
    output[0x40..0x43].copy_from_slice(&[0xc3, 0, 2]);
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

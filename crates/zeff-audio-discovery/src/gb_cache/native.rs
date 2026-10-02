use std::sync::atomic::AtomicBool;

use crate::gb_music::native::{GbBankedTiming, PreparedGbBanked};

use super::{GbCacheSong, profiles, validate_song};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbCacheSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbBanked> {
    validate_song(bytes, song, cancel)?;
    let profile = profiles::named(song.profile)
        .ok_or_else(|| anyhow::anyhow!("Cache native profile is not recognized"))?;
    let entry = profile.entry.to_le_bytes();
    let api = profile.api.to_le_bytes();
    let vblank = profile.vblank.to_le_bytes();
    let caller = usize::from(profile.caller);
    anyhow::ensure!(
        bytes[0x100..0x104] == [0, 0xc3, entry[0], entry[1]]
            && bytes[0x40..0x43] == [0xc3, vblank[0], vblank[1]]
            && bytes[caller..caller + 3] == [0xcd, api[0], api[1]],
        "Cache original entry, interrupt or selector caller differs"
    );
    let flag = profile.flag.to_le_bytes();
    let mut code = vec![
        0xf5, 0xaf, 0xea, flag[0], flag[1], 0xf1, 0xc3, entry[0], entry[1],
    ];
    let hook = (profile.reserve + code.len() as u16).to_le_bytes();
    code.extend([
        0xf3, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x07, 0xe0, 0x40, 0x31, 0xf0, 0xff, 0xe0, 0xfd,
        0x3e, 0xa5, 0xe0, 0xfc,
    ]);
    let wait_start = profile.reserve + code.len() as u16;
    code.extend([0xf0, 0xfd, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = profile.reserve + code.len() as u16;
    code.extend([
        0x3e,
        profile.index as u8,
        0xcd,
        api[0],
        api[1],
        0x3e,
        0xa5,
        0xea,
        flag[0],
        flag[1],
    ]);
    code.extend([
        0x3e, 0x91, 0xe0, 0x40, 0xaf, 0xe0, 0x0f, 0x3e, 1, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd,
    ]);
    let interrupt = (profile.reserve + code.len() as u16).to_le_bytes();
    // Startup interrupts retain the original handler until the source caller is isolated.
    code.extend([
        0xf5, 0xfa, flag[0], flag[1], 0xfe, 0xa5, 0x28, 4, 0xf1, 0xc3, vblank[0], vblank[1],
    ]);
    let tick = (profile.api + 3).to_le_bytes();
    code.extend([
        0xf1, 0xf5, 0xc5, 0xd5, 0xe5, 0xcd, tick[0], tick[1], 0xe1, 0xd1, 0xc1, 0xf1, 0xd9,
    ]);
    let start = usize::from(profile.reserve);
    let end = start + code.len();
    anyhow::ensure!(
        start >= 0x3e00 && end <= 0x4000,
        "Cache bootstrap exceeds reserved space"
    );
    for mapped in &song.mapped_spans {
        anyhow::ensure!(
            mapped.effective_offset + mapped.byte_len <= start as u32
                || mapped.effective_offset >= end as u32,
            "Cache bootstrap overlaps admitted source bytes"
        );
    }
    let mut output = bytes.to_vec();
    output[start..end].copy_from_slice(&code);
    let reserve = profile.reserve.to_le_bytes();
    output[0x100..0x103].copy_from_slice(&[0xc3, reserve[0], reserve[1]]);
    output[0x40..0x43].copy_from_slice(&[0xc3, interrupt[0], interrupt[1]]);
    output[caller..caller + 3].copy_from_slice(&[0xc3, hook[0], hook[1]]);
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

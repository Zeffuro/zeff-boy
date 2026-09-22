use super::super::{GbNativeSong, PreparedGbNative};
use super::Profile;

pub(super) fn build(
    bytes: &[u8],
    song: &GbNativeSong,
    profile: &Profile,
) -> anyhow::Result<PreparedGbNative> {
    let mut code = vec![
        0xf3,
        0x31,
        0xf0,
        0xdf,
        0xaf,
        0xe0,
        0xff,
        0xe0,
        0x0f,
        0xe0,
        0x40,
        0xe0,
        0x07,
        0x3e,
        1,
        0xe0,
        0x70,
        0xe0,
        0x4d,
        0x10,
        0,
        0x21,
        0,
        0xc0,
        0x01,
        0,
        0x20,
        0xaf,
        0x22,
        0x0b,
        0x78,
        0xb1,
        0x20,
        0xf9,
        0x21,
        0x80,
        0xff,
        0x06,
        0x7f,
        0xaf,
        0x22,
        0x05,
        0x20,
        0xfc,
        0xcd,
        profile.init as u8,
        (profile.init >> 8) as u8,
        0xaf,
        0xe0,
        7,
        0xe0,
        0x0f,
        0x3e,
        0xa5,
        0xe0,
        0x81,
    ];
    let wait_start = 0x150_u16 + u16::try_from(code.len())?;
    code.extend([0xf0, 0x80, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150_u16 + u16::try_from(code.len())?;
    code.extend([
        0xaf,
        0xe0,
        4,
        0xe0,
        0x0f,
        0xcd,
        profile.timer_start as u8,
        (profile.timer_start >> 8) as u8,
        0x3e,
        song.raw_index,
        0xcd,
        profile.queue as u8,
        (profile.queue >> 8) as u8,
        0x3e,
        4,
        0xe0,
        0xff,
        0xfb,
        0x76,
        0x18,
        0xfd,
    ]);
    let patch = song.native.bootstrap;
    let patch_start = patch.effective_offset as usize;
    anyhow::ensure!(
        patch.canonical_cpu_address == 0x150
            && patch.byte_len == 0x100
            && code.len() <= patch.byte_len as usize
            && bytes
                .get(patch_start..patch_start + patch.byte_len as usize)
                .is_some(),
        "GB timer bootstrap does not fit its qualified startup window"
    );
    let hook = song.native.startup_hook.effective_offset as usize;
    anyhow::ensure!(
        song.native.startup_hook.canonical_cpu_address == 0x100
            && song.native.startup_hook.byte_len == 3
            && bytes.get(hook..hook + 3).is_some(),
        "GB timer bootstrap startup hook is invalid"
    );
    let mut result = bytes.to_vec();
    result[hook..hook + 3].copy_from_slice(&[0xc3, 0x50, 1]);
    result[patch_start..patch_start + code.len()].copy_from_slice(&code);
    Ok(PreparedGbNative {
        bytes: result,
        timing: song.native.timing,
        ready_address: 0xff81,
        ready_value: 0xa5,
        ack_address: 0xff80,
        ack_value: 0x5a,
        wait_start,
        wait_end,
        playback_frames: song.playback_frames,
        playback_clocks: song.playback_clocks,
    })
}

use super::super::{GbNativeSong, PreparedGbNative};

pub(super) fn build(
    bytes: &[u8],
    song: &GbNativeSong,
    selector: u16,
) -> anyhow::Result<PreparedGbNative> {
    let patch = song.native.bootstrap;
    let cpu = u16::try_from(patch.canonical_cpu_address)?;
    let tick = u16::try_from(song.native.tick.canonical_cpu_address)?;
    let code = [
        0xf3,
        0xaf,
        0xe0,
        0x0f,
        0xe0,
        0xff,
        0x3e,
        0xa5,
        0xe0,
        0xfc,
        0xf0,
        0xfb,
        0xfe,
        0x5a,
        0x20,
        0xfa,
        0x3e,
        song.raw_index,
        0xcd,
        selector as u8,
        (selector >> 8) as u8,
        0xaf,
        0xe0,
        0x0f,
        0x3e,
        1,
        0xe0,
        0xff,
        0xfb,
        0x76,
        0xcd,
        tick as u8,
        (tick >> 8) as u8,
        0x18,
        0xfa,
    ];
    let offset = patch.effective_offset as usize;
    anyhow::ensure!(
        code.len() <= patch.byte_len as usize
            && u32::from(cpu) + patch.byte_len <= 0x4000
            && bytes
                .get(offset..offset + patch.byte_len as usize)
                .is_some_and(|window| window.iter().all(|&byte| byte == 0xff)),
        "GB bootstrap exceeds its qualified unused patch window"
    );
    let mut result = bytes.to_vec();
    result[offset..offset + code.len()].copy_from_slice(&code);
    let hook = song.native.startup_hook.effective_offset as usize;
    result[hook..hook + 3].copy_from_slice(&[0xc3, cpu as u8, (cpu >> 8) as u8]);
    Ok(PreparedGbNative {
        bytes: result,
        timing: song.native.timing,
        ready_address: 0xfffc,
        ready_value: 0xa5,
        ack_address: 0xfffb,
        ack_value: 0x5a,
        wait_start: cpu + 10,
        wait_end: cpu + 16,
        playback_frames: song.playback_frames,
        playback_clocks: song.playback_clocks,
    })
}

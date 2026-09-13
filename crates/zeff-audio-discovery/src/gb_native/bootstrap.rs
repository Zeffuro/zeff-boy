use super::{GbNativeSong, PreparedGbNative};

pub(super) const READY_ADDRESS: u16 = 0xfffc;
pub(super) const ACK_ADDRESS: u16 = 0xfffb;

pub(super) fn build(bytes: &[u8], song: &GbNativeSong) -> anyhow::Result<PreparedGbNative> {
    let patch = song.native.bootstrap;
    let cpu = u16::try_from(patch.canonical_cpu_address)?;
    let mut code = vec![
        0xf3, 0xaf, 0xe0, 0x0f, 0xea, 0xff, 0xff, 0x3e, 0xa5, 0xe0, 0xfc,
    ];
    let wait = code.len();
    code.extend_from_slice(&[0xf0, 0xfb, 0xfe, 0x5a, 0x20, 0xfa]);
    code.extend_from_slice(&[0x0e, song.bank, 0x3e, song.raw_index, 0xcd, 0xa1, 0x23]);
    code.extend_from_slice(&[
        0x3e, song.bank, 0xe0, 0xb8, 0xea, 0, 0x20, 0xaf, 0xe0, 0x0f, 0x3e, 1, 0xea, 0xff, 0xff,
        0xfb, 0x76, 0x18, 0xfd,
    ]);
    let irq = cpu
        .checked_add(u16::try_from(code.len())?)
        .ok_or_else(|| anyhow::anyhow!("GB interrupt address overflow"))?;
    code.extend_from_slice(&[0xf5, 0xc5, 0xd5, 0xe5, 0xcd, 0xcb, 0x28, 0xcd]);
    code.extend_from_slice(&u16::try_from(song.native.tick.canonical_cpu_address)?.to_le_bytes());
    code.extend_from_slice(&[0xe1, 0xd1, 0xc1, 0xf1, 0xd9]);
    let offset = patch.effective_offset as usize;
    anyhow::ensure!(
        code.len() <= patch.byte_len as usize
            && u32::from(cpu) + patch.byte_len <= 0x4000
            && bytes
                .get(offset..offset + patch.byte_len as usize)
                .is_some_and(|window| window.iter().all(|&byte| byte == 0)),
        "GB bootstrap exceeds its qualified unused patch window"
    );
    let mut result = bytes.to_vec();
    result[offset..offset + code.len()].copy_from_slice(&code);
    let hook = song.native.startup_hook.effective_offset as usize;
    result[hook..hook + 3].copy_from_slice(&[0xc3, cpu as u8, (cpu >> 8) as u8]);
    result[0x40..0x43].copy_from_slice(&[0xc3, irq as u8, (irq >> 8) as u8]);
    Ok(PreparedGbNative {
        bytes: result,
        timing: song.native.timing,
        ready_address: READY_ADDRESS,
        ready_value: 0xa5,
        ack_address: ACK_ADDRESS,
        ack_value: 0x5a,
        wait_start: cpu + wait as u16,
        wait_end: cpu + wait as u16 + 6,
        playback_frames: song.playback_frames,
        playback_clocks: song.playback_clocks,
    })
}

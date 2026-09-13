use super::{NesNativeSong, PreparedNesNative};

pub(super) fn build(bytes: &[u8], song: &NesNativeSong) -> anyhow::Result<PreparedNesNative> {
    let mut code = vec![0xa9, 1, 0x8d, 0xf0, 7];
    let wait = 0xff40 + code.len() as u16;
    code.extend_from_slice(&[0xad, 0xf1, 7, 0xc9, 1, 0xd0, 0xf9]);
    code.extend_from_slice(&[0xa9, song.raw_index, 0x8d, 0xf5, 6]);
    let play = 0xff40 + code.len() as u16;
    code.extend_from_slice(&[0x20, 0x59, 0xb9, 0x4c]);
    code.extend_from_slice(&play.to_le_bytes());
    anyhow::ensure!(
        code.len() <= song.native.bootstrap.byte_len as usize,
        "NES bootstrap exceeds its qualified patch window"
    );
    let mut result = bytes.to_vec();
    let offset = song.native.bootstrap.effective_offset as usize;
    result[offset..offset + code.len()].copy_from_slice(&code);
    result[0xb6..0xb9].copy_from_slice(&[0x4c, 0x40, 0xff]);
    Ok(PreparedNesNative {
        bytes: result,
        mapper: song.native.mapper,
        timing: song.native.timing,
        ready_address: 0x07f0,
        ack_address: 0x07f1,
        wait_start: wait,
        wait_end: wait + 7,
    })
}

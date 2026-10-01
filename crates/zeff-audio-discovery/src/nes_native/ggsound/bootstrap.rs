use super::super::{NesNativeSong, NesNativeTiming, PreparedNesNative};
use super::recognition::BOOTSTRAP;

pub(super) fn build(bytes: &[u8], song: &NesNativeSong) -> anyhow::Result<PreparedNesNative> {
    let init = u16::try_from(song.native.init.canonical_cpu_address)?;
    let header = u16::try_from(song.native.tables.canonical_cpu_address)?;
    let mut code = vec![
        0x78, 0xd8, 0xa9, 0, 0x8d, 0, 0x20, 0x8d, 1, 0x20, 0x8d, 0x15, 0x40, 0xa2, 0xff, 0x9a,
        0xa9, 0, 0xa2, 0,
    ];
    for page in 0..8 {
        code.extend_from_slice(&[0x9d, 0, page]);
    }
    code.extend_from_slice(&[0xe8, 0xd0, 0xe5, 0xa9, 1, 0x8d, 0xf0, 7]);
    let wait = BOOTSTRAP + code.len() as u16;
    code.extend_from_slice(&[
        0xad,
        0xf1,
        7,
        0xc9,
        1,
        0xd0,
        0xf9,
        0xa2,
        0x40,
        0x8e,
        0x17,
        0x40,
        0xa9,
        1,
        0xa2,
        header as u8,
        0xa0,
        (header >> 8) as u8,
        0x20,
    ]);
    code.extend_from_slice(&init.to_le_bytes());
    code.extend_from_slice(&[0xa9, song.raw_index, 0x20]);
    code.extend_from_slice(&(init + 0x1d).to_le_bytes());
    code.extend_from_slice(&[0x2c, 2, 0x20, 0xa9, 0x80, 0x8d, 0, 0x20]);
    let idle = BOOTSTRAP + code.len() as u16;
    code.push(0x4c);
    code.extend_from_slice(&idle.to_le_bytes());
    let nmi = BOOTSTRAP + code.len() as u16;
    code.push(0x20);
    code.extend_from_slice(&u16::try_from(song.native.tick.canonical_cpu_address)?.to_le_bytes());
    code.extend_from_slice(&[0x40, 0x40]);
    anyhow::ensure!(
        code.len() <= 256,
        "GGSound bootstrap exceeds its patch window"
    );
    let mut result = bytes.to_vec();
    result[0x7810..0x7810 + code.len()].copy_from_slice(&code);
    for (slot, address) in result[0x800a..0x8010]
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip([nmi, BOOTSTRAP, nmi + 4])
    {
        slot.copy_from_slice(&address.to_le_bytes());
    }
    Ok(PreparedNesNative {
        bytes: result,
        mapper: 0,
        timing: NesNativeTiming::Ntsc,
        ready_address: 0x7f0,
        ack_address: 0x7f1,
        wait_start: wait,
        wait_end: wait + 7,
    })
}

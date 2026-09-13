use super::{NesNativeSong, NesNativeTiming, PreparedNesNative};

pub(super) const READY_ADDRESS: u16 = 0x07f0;
pub(super) const ACK_ADDRESS: u16 = 0x07f1;

pub(super) fn build(bytes: &[u8], song: &NesNativeSong) -> anyhow::Result<PreparedNesNative> {
    let patch = song.native.bootstrap;
    let cpu = u16::try_from(patch.canonical_cpu_address)?;
    let mut code = vec![0x78, 0xd8, 0xa2, 0xff, 0x9a, 0xa9, 0, 0xa2, 0];
    for page in 0..8 {
        code.extend_from_slice(&[0x9d, 0, page]);
    }
    code.extend_from_slice(&[0xe8, 0xd0, 0xe5]);
    code.extend_from_slice(&[0xa9, 1, 0x8d]);
    code.extend_from_slice(&READY_ADDRESS.to_le_bytes());
    let wait = code.len();
    code.push(0xad);
    code.extend_from_slice(&ACK_ADDRESS.to_le_bytes());
    code.extend_from_slice(&[0xc9, 1, 0xd0, 0xf9]);
    code.extend_from_slice(&[0xa9, 0x1f, 0x8d, 0x15, 0x40, 0xa9, 0xc0, 0x8d, 0x17, 0x40]);
    code.extend_from_slice(&[0xa9, song.raw_index, 0x20]);
    code.extend_from_slice(&u16::try_from(song.native.init.canonical_cpu_address)?.to_le_bytes());
    code.extend_from_slice(&[0xa9, 0x80, 0x8d, 0, 0x20, 0x58]);
    let idle = cpu
        .checked_add(u16::try_from(code.len())?)
        .ok_or_else(|| anyhow::anyhow!("NES bootstrap address overflow"))?;
    code.push(0x4c);
    code.extend_from_slice(&idle.to_le_bytes());
    let nmi = cpu
        .checked_add(u16::try_from(code.len())?)
        .ok_or_else(|| anyhow::anyhow!("NES interrupt address overflow"))?;
    code.extend_from_slice(&[0x48, 0x8a, 0x48, 0x98, 0x48, 0x20]);
    code.extend_from_slice(&u16::try_from(song.native.tick.canonical_cpu_address)?.to_le_bytes());
    code.extend_from_slice(&[0x68, 0xa8, 0x68, 0xaa, 0x68, 0x40]);
    anyhow::ensure!(
        code.len() <= patch.byte_len as usize && u32::from(cpu) + patch.byte_len <= 0xfffa,
        "NES bootstrap exceeds its qualified patch window"
    );
    let mut result = bytes.to_vec();
    let offset = patch.effective_offset as usize;
    result
        .get_mut(offset..offset + code.len())
        .ok_or_else(|| anyhow::anyhow!("NES bootstrap is outside its source"))?
        .copy_from_slice(&code);
    let vectors = result
        .get_mut(0x800a..0x8010)
        .ok_or_else(|| anyhow::anyhow!("NES interrupt vectors are missing"))?;
    vectors[..2].copy_from_slice(&nmi.to_le_bytes());
    vectors[2..4].copy_from_slice(&cpu.to_le_bytes());
    vectors[4..].copy_from_slice(&(nmi + 13).to_le_bytes());
    Ok(PreparedNesNative {
        bytes: result,
        mapper: song.native.mapper,
        timing: NesNativeTiming::Ntsc,
        ready_address: READY_ADDRESS,
        ack_address: ACK_ADDRESS,
        wait_start: cpu + wait as u16,
        wait_end: cpu + wait as u16 + 7,
    })
}

use anyhow::{Result, ensure};

use crate::nes_native::{NesNativeTiming, PreparedNesNative};

use super::{
    NesToseSong,
    closed::{Input, Profile},
};

pub(super) fn build(
    bytes: &[u8],
    song: &NesToseSong,
    profile: &Profile,
) -> Result<PreparedNesNative> {
    let mut code = vec![0x78, 0xd8, 0xa2, 0xff, 0x9a, 0xa9, 0, 0xa2, 0];
    for page in 0..8 {
        code.extend([0x9d, 0, page]);
    }
    code.extend([0xe8, 0xd0, 0xe5, 0x8d, 0, 0x20, 0x8d, 1, 0x20]);
    code.extend([
        0x8d,
        0x10,
        0x40,
        0xa9,
        profile.frame_control,
        0x8d,
        0x17,
        0x40,
    ]);
    match profile.mapper {
        1 => {
            code.extend([0xa9, 0x80, 0x8d, 0, 0x80]);
            for (address, value) in [(0x8000_u16, 0x0c), (0xe000, profile.bank)] {
                for bit in 0..5 {
                    code.extend([0xa9, (value >> bit) & 1, 0x8d]);
                    code.extend(address.to_le_bytes());
                }
            }
        }
        3 => (),
        4 => {
            code.extend([0xa9, 0, 0x8d, 0, 0xe0]);
            for register in 6..=7 {
                code.extend([0xa9, register, 0x8d, 0, 0x80]);
                code.extend([0xa9, profile.bank * 2 + register - 6, 0x8d, 1, 0x80]);
            }
        }
        16 => {
            code.extend([0xa9, 0, 0x8d, 0x0a, 0x80]);
            code.extend([0xa9, profile.bank, 0x8d, 8, 0x80]);
        }
        _ => anyhow::bail!("unsupported closed NES TOSE mapper"),
    }
    call(&mut code, profile.init);
    code.extend([0xa9, 1, 0x8d, 0xf0, 7]);
    let wait_start = 0xf800 + code.len() as u16;
    code.extend([0xad, 0xf1, 7, 0xc9, 1, 0xd0, 0xf9]);
    for channel in 0..4 {
        let index = song.index as u8 + channel;
        match profile.input {
            Input::Accumulator => code.extend([0xa9, index]),
            Input::Y => code.extend([0xa0, index]),
            Input::Memory(address) => {
                code.extend([0xa9, index, 0x8d]);
                code.extend(address.to_le_bytes());
            }
        }
        call(&mut code, profile.selector);
    }
    code.extend([0x2c, 2, 0x20, 0xa9, 0x80, 0x8d, 0, 0x20]);
    let idle = 0xf800 + code.len() as u16;
    code.push(0x4c);
    code.extend(idle.to_le_bytes());
    let nmi = 0xf800 + code.len() as u16;
    code.extend([0x48, 0x8a, 0x48, 0x98, 0x48]);
    call(&mut code, profile.tick);
    code.extend([0x68, 0xa8, 0x68, 0xaa, 0x68, 0x40]);
    ensure!(
        code.len() < 256,
        "NES TOSE bootstrap exceeds its isolated window"
    );
    let mut result = bytes.to_vec();
    let at = profile.span(0xf800, 256).effective_offset as usize;
    result[at..at + code.len()].copy_from_slice(&code);
    let vectors = profile.span(0xfffa, 6).effective_offset as usize;
    result[vectors..vectors + 2].copy_from_slice(&nmi.to_le_bytes());
    result[vectors + 2..vectors + 4].copy_from_slice(&0xf800_u16.to_le_bytes());
    result[vectors + 4..vectors + 6].copy_from_slice(&(nmi + 13).to_le_bytes());
    for span in &song.mapped_spans {
        let start = span.effective_offset as usize;
        let end = start + span.byte_len as usize;
        ensure!(
            bytes[start..end] == result[start..end],
            "NES TOSE bootstrap overlaps source assets"
        );
    }
    Ok(PreparedNesNative {
        bytes: result,
        mapper: profile.mapper,
        timing: NesNativeTiming::Ntsc,
        ready_address: 0x7f0,
        ack_address: 0x7f1,
        wait_start,
        wait_end: wait_start + 7,
    })
}

fn call(code: &mut Vec<u8>, address: u16) {
    code.push(0x20);
    code.extend(address.to_le_bytes());
}

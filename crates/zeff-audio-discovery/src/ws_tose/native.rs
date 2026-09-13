use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};

use super::{PreparedWsTose, WsToseSong};

pub fn prepare_rom(bytes: &[u8], song: &WsToseSong, cancel: &AtomicBool) -> Result<PreparedWsTose> {
    if song.profile.starts_with("ws-tose-fixed-") {
        return super::legacy::prepare_rom(bytes, song, cancel);
    }
    let profile = super::checked_profile(bytes, song, cancel)?;
    ensure!(
        u32::from(profile.slots) + 8 * 0x34 < 0x3d00,
        "WonderSwan driver state overlaps bootstrap RAM"
    );
    let bootstrap = bytes.len() - 0x2000;
    ensure!(
        song.mapped_spans.iter().all(|span| {
            let start = span.effective_offset as usize;
            let end = start + span.byte_len as usize;
            end <= bootstrap || start >= bootstrap + 512
        }),
        "WonderSwan bootstrap overlaps selected source data"
    );
    let mut code = vec![
        0xfa, 0xfc, 0x33, 0xc0, 0x8e, 0xd0, 0xbc, 0xf0, 0x3d, 0x8e, 0xd8, 0x8e, 0xc0, 0x33, 0xff,
        0xb9, 0x00, 0x20, 0xf3, 0xab, 0xe6, 0xb2,
    ];
    far_call(&mut code, profile.init, profile.segment);
    far_call(&mut code, profile.status, profile.segment);
    code.extend([0xc6, 0x06, 0x00, 0x3e, 1]);
    let wait_start = 0xfe000 + code.len() as u32;
    code.extend([0x80, 0x3e, 0x01, 0x3e, 1, 0x75, 0xf9]);
    code.push(0xb8);
    code.extend(song.index.to_le_bytes());
    far_call(&mut code, profile.selector, profile.segment);
    code.extend([0xc7, 0x06, 0x38, 0]);
    let vector_fixup = code.len();
    code.extend([0, 0, 0xc7, 0x06, 0x3a, 0, 0, 0xf0]);
    code.extend([
        0xb0, 8, 0xe6, 0xb0, 0xb0, 0x40, 0xe6, 0xb6, 0xe6, 0xb2, 0xfb,
    ]);
    let idle = 0xe000 + code.len() as u16;
    code.extend([0xf4, 0xeb, 0xfd]);
    let interrupt = 0xe000 + code.len() as u16;
    code.extend([0x50, 0xb0, 0x40, 0xe6, 0xb6]);
    far_call(&mut code, profile.tick, profile.segment);
    far_call(&mut code, profile.status, profile.segment);
    code.extend([0x58, 0xcf]);
    code[vector_fixup..vector_fixup + 2].copy_from_slice(&interrupt.to_le_bytes());
    ensure!(
        code.len() < 512 && idle < 0xe1f0,
        "WonderSwan bootstrap exceeds its isolated window"
    );
    let mut result = bytes.to_vec();
    let footer_model = result.len() - 9;
    result[footer_model] = match song.hardware {
        super::WsToseHardware::Mono => 0,
        super::WsToseHardware::Color => 1,
    };
    result[bootstrap..bootstrap + code.len()].copy_from_slice(&code);
    let reset = result.len() - 16;
    result[reset..reset + 5].copy_from_slice(&[0xea, 0, 0xe0, 0, 0xf0]);
    let checksum = result[..result.len() - 2]
        .iter()
        .fold(0_u16, |sum, &byte| sum.wrapping_add(u16::from(byte)));
    let end = result.len();
    result[end - 2..].copy_from_slice(&checksum.to_le_bytes());
    Ok(PreparedWsTose {
        bytes: result,
        bootstrap: super::WsToseBootstrap::Cartridge,
        hardware: song.hardware,
        ready_address: 0x3e00,
        ack_address: 0x3e01,
        wait_start,
        wait_end: wait_start + 7,
    })
}

fn far_call(code: &mut Vec<u8>, offset: u16, segment: u16) {
    code.push(0x9a);
    code.extend(offset.to_le_bytes());
    code.extend(segment.to_le_bytes());
}

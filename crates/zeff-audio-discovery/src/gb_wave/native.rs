use std::sync::atomic::AtomicBool;

use crate::gb_music::native::{GbBankedTiming, PreparedGbBanked};

use super::{GbWaveSong, profiles, validate_song};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbWaveSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbBanked> {
    validate_song(bytes, song, cancel)?;
    let p = profiles::named(song.profile)
        .ok_or_else(|| anyhow::anyhow!("Wave-state native profile is not recognized"))?;
    let mut code = vec![
        0xf3, 0x31, 0xfe, 0xff, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x07, 0xe0, 0x40, 0x3e, 1,
        0xe0, 0x4d, 0x10, 0, 0xaf, 0xe0, 0xfb, 0x3e, 0xa5, 0xe0, 0xfc,
    ];
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0xfb, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    code.extend([0x3e, 1, 0xe0, 0x70, 0xaf, 0xea, 0, 0x30]);
    // The original reset erases WRAM; its return stack must remain in HRAM.
    call(&mut code, p.reset);
    code.extend([0x31, 0, 0xcf]);
    bank(&mut code, p.init_bank);
    call(&mut code, 0x4000);
    code.extend([0x3e, p.index]);
    if p.bank_setter != 0 {
        call(&mut code, p.bank_setter);
    }
    bank(&mut code, p.bank);
    code.extend([0x3e, p.index]);
    call(&mut code, 0x4006);
    if p.timer {
        code.extend([
            0xaf, 0xe0, 0xfa, 0xe0, 0x04, 0x3e, 0x78, 0xe0, 0x06, 0xe0, 0x05, 0x3e, 7, 0xe0, 0x07,
        ]);
    } else {
        code.extend([0x3e, 0x91, 0xe0, 0x40]);
    }
    code.extend([
        0xaf,
        0xe0,
        0x0f,
        0x3e,
        if p.timer { 4 } else { 1 },
        0xe0,
        0xff,
        0xfb,
        0x76,
        0x18,
        0xfd,
    ]);
    let irq = 0x150 + code.len() as u16;
    code.extend([0xf5, 0xc5, 0xd5, 0xe5]);
    if p.timer {
        code.extend([0xf0, 0xfa, 0xe6, 3, 0x20, 0x0a]);
    }
    bank(&mut code, p.bank);
    call(&mut code, 0x4003);
    if p.timer {
        code.extend([0xf0, 0xfa, 0x3c, 0xe0, 0xfa]);
    }
    code.extend([0xe1, 0xd1, 0xc1, 0xf1, 0xd9]);
    let end = 0x150 + code.len() as u32;
    anyhow::ensure!(end <= 0x300, "Wave-state bootstrap exceeds reserved space");
    let vector = if p.timer { 0x50u32 } else { 0x40u32 };
    for mapped in &song.mapped_spans {
        let start = mapped.effective_offset;
        let source_end = start + mapped.byte_len;
        for (a, b) in [(vector, vector + 3), (0x100, 0x103), (0x150, end)] {
            anyhow::ensure!(
                source_end <= a || start >= b,
                "Wave-state bootstrap overlaps admitted source bytes"
            );
        }
    }
    let mut output = bytes.to_vec();
    output[0x150..end as usize].copy_from_slice(&code);
    output[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    output[vector as usize..vector as usize + 3].copy_from_slice(&[
        0xc3,
        irq as u8,
        (irq >> 8) as u8,
    ]);
    Ok(PreparedGbBanked {
        bytes: output,
        timing: GbBankedTiming::CgbDouble,
        ready_address: 0xfffc,
        ready_value: 0xa5,
        ack_address: 0xfffb,
        ack_value: 0x5a,
        wait_start,
        wait_end,
    })
}

fn call(code: &mut Vec<u8>, address: u16) {
    code.extend([0xcd, address as u8, (address >> 8) as u8]);
}

fn bank(code: &mut Vec<u8>, bank: u8) {
    code.extend([0x3e, bank, 0xea, 0, 0x20, 0xe0, 0x80]);
}

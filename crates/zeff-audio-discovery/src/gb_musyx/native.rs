use std::sync::atomic::AtomicBool;

use super::{GbMusyxSong, PreparedGbMusyx, checked_driver};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbMusyxSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbMusyx> {
    let driver = checked_driver(bytes, song, cancel)?;
    let profile = driver.profile;
    let mut output = bytes.to_vec();
    let mut code = vec![
        0xf3, 0x31, 0x00, 0xd0, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0x3e, 1, 0xe0, 0x70,
    ];
    bank(&mut code, driver.bank, profile.current_bank);
    code.extend([0x0e, 0xfe, 0x3e, 0x81]);
    call(&mut code, profile.init);
    code.extend([
        0x3e,
        driver.bank,
        0xea,
        if profile.current_bank == 0xdf07 { 6 } else { 0 },
        0xdf,
    ]);
    bank(&mut code, driver.bank, profile.current_bank);
    code.extend([0xaf, 0xe0, 0xfb, 0x3e, 0xa5, 0xe0, 0xfc]);
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0xfb, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    code.extend([0x3e, song.index as u8]);
    call(&mut code, profile.start_song);
    code.extend([
        0xaf, 0xe0, 0x0f, 0xf0, 0xff, 0xf6, 1, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd,
    ]);
    anyhow::ensure!(
        code.len() < 0xb0,
        "native startup exceeds its reserved space"
    );
    output[0x150..0x150 + code.len()].copy_from_slice(&code);
    output[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    for (address, vector, target, nested) in [
        (0x200, 0x40, profile.handle, true),
        (0x250, 0x50, profile.sample, false),
    ] {
        let [lo, hi] = profile.current_bank.to_le_bytes();
        let mut irq = vec![0xf5, 0xc5, 0xd5, 0xe5, 0xfa, lo, hi, 0xf5];
        bank(&mut irq, driver.bank, profile.current_bank);
        if nested {
            irq.push(0xfb);
        }
        call(&mut irq, target);
        irq.extend([
            0xf3, 0xf1, 0xea, lo, hi, 0xea, 0, 0x21, 0xe1, 0xd1, 0xc1, 0xf1, 0xd9,
        ]);
        output[address..address + irq.len()].copy_from_slice(&irq);
        output[vector..vector + 3].copy_from_slice(&[0xc3, address as u8, (address >> 8) as u8]);
    }
    Ok(PreparedGbMusyx {
        bytes: output,
        ready_address: 0xfffc,
        ready_value: 0xa5,
        ack_address: 0xfffb,
        ack_value: 0x5a,
        wait_start,
        wait_end,
    })
}

fn call(code: &mut Vec<u8>, address: u16) {
    let [lo, hi] = address.to_le_bytes();
    code.extend([0xcd, lo, hi]);
}

fn bank(code: &mut Vec<u8>, bank: u8, shadow: u16) {
    let [lo, hi] = shadow.to_le_bytes();
    code.extend([0x3e, bank, 0xea, lo, hi, 0xea, 0, 0x21]);
}

use std::sync::atomic::AtomicBool;

use super::{GbToseSong, PreparedGbTose, checked_driver};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbToseSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbTose> {
    let driver = checked_driver(bytes, song, cancel)?;
    let mut output = bytes.to_vec();
    let mut code = vec![0xf3, 0x31, 0x00, 0xd0, 0xaf, 0xe0, 0xff, 0xe0, 0x0f];
    code.extend([0xea, 0, 0x60, 0x3e, song.bank >> 5, 0xea, 0, 0x40]);
    code.extend([0x3e, song.bank & 31, 0xea, 0, 0x20]);
    call(&mut code, driver.start as u16);
    code.extend([0xaf, 0xe0, 0x80, 0x3e, 0xa5, 0xe0, 0x81]);
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0x80, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    code.extend([0x3e, song.index as u8]);
    call(&mut code, driver.start as u16 + driver.profile.selector);
    code.extend([
        0xaf, 0xe0, 0x0f, 0x3e, 1, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd,
    ]);
    anyhow::ensure!(
        code.len() < 0xb0,
        "native startup exceeds its reserved space"
    );
    output[0x150..0x150 + code.len()].copy_from_slice(&code);
    output[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    let mut irq = vec![0xf5, 0xc5, 0xd5, 0xe5];
    call(&mut irq, driver.start as u16 + driver.profile.tick);
    irq.extend([0xe1, 0xd1, 0xc1, 0xf1, 0xd9]);
    output[0x200..0x200 + irq.len()].copy_from_slice(&irq);
    output[0x40..0x43].copy_from_slice(&[0xc3, 0, 2]);
    Ok(PreparedGbTose {
        bytes: output,
        ready_address: 0xff81,
        ready_value: 0xa5,
        ack_address: 0xff80,
        ack_value: 0x5a,
        wait_start,
        wait_end,
    })
}

fn call(code: &mut Vec<u8>, target: u16) {
    let [lo, hi] = target.to_le_bytes();
    code.extend([0xcd, lo, hi]);
}

use std::sync::atomic::AtomicBool;

use super::{GbQuickThunderSong, PreparedGbQuickThunder, checked_driver};

pub fn prepare_rom(
    bytes: &[u8],
    song: &GbQuickThunderSong,
    cancel: &AtomicBool,
) -> anyhow::Result<PreparedGbQuickThunder> {
    let driver = checked_driver(bytes, song, cancel)?;
    let sampled = super::sampled::is_sampled(driver);
    let isolated = matches!(bytes[0x147], 0x97 | 0x99);
    let mut output = if isolated {
        super::isolated::project(bytes, song)?
    } else {
        bytes.to_vec()
    };
    let stack = if driver.wram >= 0xdf40 {
        0xd000u16
    } else {
        0xdff0
    };
    let [stack_lo, stack_hi] = stack.to_le_bytes();
    let mut code = vec![0xf3, 0x31, stack_lo, stack_hi, 0xaf, 0xe0, 0xff, 0xe0, 0x0f];
    code.extend([0x3e, 1, 0xe0, 0x4d, 0x10, 0]);
    if !isolated {
        code.extend([0x3e, (song.bank >> 8) as u8, 0xea, 0, 0x30]);
        code.extend([0x3e, song.bank as u8, 0xea, 0, 0x20]);
    }
    let [lo, hi] = driver.wram.to_le_bytes();
    code.extend([0x21, lo, hi, 0x06, 0x80, 0xaf, 0x22, 0x05, 0x20, 0xfc]);
    if sampled {
        code.extend([0x3e, song.bank as u8, 0xe0, 0x88]);
        code.extend([
            0x21, 0x06, 0xca, 0x36, 0xc3, 0x23, 0x36, 0x40, 0x23, 0x36, 0x17,
        ]);
    }
    code.extend([0xaf, 0xe0, 0x80, 0x3e, 0xa5, 0xe0, 0x81]);
    let wait_start = 0x150 + code.len() as u16;
    code.extend([0xf0, 0x80, 0xfe, 0x5a, 0x20, 0xfa]);
    let wait_end = 0x150 + code.len() as u16;
    if sampled {
        code.extend([0xaf, 0xe0, 0x26, 0x3e, 0x80, 0xe0, 0x26]);
    }
    code.extend([0x1e, song.index as u8]);
    if sampled {
        code.extend([0x06, 0xe0]);
    }
    call(&mut code, driver.profile.selector);
    if sampled {
        code.extend([0x3e, 1, 0xea, 0x6e, 0xc1]);
    }
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
    call(&mut irq, driver.profile.tick);
    irq.extend([0xe1, 0xd1, 0xc1, 0xf1, 0xd9]);
    output[0x200..0x200 + irq.len()].copy_from_slice(&irq);
    output[0x40..0x43].copy_from_slice(&[0xc3, 0, 2]);
    Ok(PreparedGbQuickThunder {
        bytes: output,
        hardware: song.hardware,
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

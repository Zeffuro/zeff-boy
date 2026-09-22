use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};

use super::{DecodedStream, FrameRate};

pub(super) fn encode(
    stream: &DecodedStream,
    rate: FrameRate,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let ticks_per_frame = 44_100 / rate.hz();
    let mut bytes = vec![0; 0x100];
    bytes[..4].copy_from_slice(b"Vgm ");
    put(&mut bytes, 8, 0x171);
    put(&mut bytes, 0x0c, 3_579_545);
    put(&mut bytes, 0x18, stream.frames * ticks_per_frame);
    put(&mut bytes, 0x24, rate.hz());
    bytes[0x28] = 9;
    bytes[0x2a] = 16;
    bytes[0x2b] = 4;
    put(&mut bytes, 0x34, 0x100 - 0x34);
    for value in [0x9f, 0xbf, 0xdf, 0xff] {
        bytes.extend([0x50, value]);
    }
    let mut frame = 0;
    for write in &stream.writes {
        ensure!(!cancel.load(Ordering::Relaxed), "PSGlib export cancelled");
        wait(&mut bytes, (write.frame - frame) * ticks_per_frame);
        bytes.extend([0x50, write.value]);
        frame = write.frame;
    }
    wait(&mut bytes, (stream.frames - frame) * ticks_per_frame);
    bytes.push(0x66);
    let size = (bytes.len() - 4) as u32;
    put(&mut bytes, 4, size);
    Ok(bytes)
}

fn wait(bytes: &mut Vec<u8>, mut ticks: u32) {
    while ticks > 0 {
        let amount = ticks.min(u32::from(u16::MAX)) as u16;
        bytes.push(0x61);
        bytes.extend(amount.to_le_bytes());
        ticks -= u32::from(amount);
    }
}

fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

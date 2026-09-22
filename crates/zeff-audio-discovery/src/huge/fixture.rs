use std::sync::atomic::AtomicBool;

use super::{discovery, isolation};

pub fn rom() -> Vec<u8> {
    let mut bytes = song();
    let start = 0x1800u16;
    let update = discovery::fixture_driver(&mut bytes, start, 0xc000);
    bytes[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    bytes[0x150..0x157].copy_from_slice(&[0x21, 0, 2, 0xcd, 0, 0x18, 0xc9]);
    bytes[0x40..0x44].copy_from_slice(&[0xcd, update as u8, (update >> 8) as u8, 0xd9]);
    let cancel = AtomicBool::new(false);
    let report = discovery::discover(&bytes, Default::default(), &cancel).unwrap();
    isolation::build(&bytes, &report.bound[0], 0, &cancel)
        .unwrap()
        .bytes
}

fn word_at(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn song() -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    bytes[0x200] = 1;
    for (index, pointer) in [
        0x220, 0x230, 0x230, 0x230, 0x230, 0x300, 0x306, 0x30c, 0, 0x320,
    ]
    .into_iter()
    .enumerate()
    {
        word_at(&mut bytes, 0x201 + index * 2, pointer);
    }
    bytes[0x220] = 4;
    word_at(&mut bytes, 0x230, 0x1000);
    word_at(&mut bytes, 0x232, 0x1100);
    for (at, pitch) in [(0x1000, 24), (0x1100, 36)] {
        for row in 0..64 {
            bytes[at + row * 3] = 90;
        }
        bytes[at..at + 3].copy_from_slice(&[pitch, 0x10, 0]);
        bytes[at + 48..at + 51].copy_from_slice(&[pitch + 7, 0x10, 0]);
    }
    bytes[0x300..0x306].copy_from_slice(&[0, 0x80, 0xf0, 0, 0, 0x80]);
    bytes[0x306..0x30c].copy_from_slice(&[0, 0x20, 0, 0, 0, 0x80]);
    bytes[0x30c..0x312].copy_from_slice(&[0xf0, 0, 0, 0, 0, 0]);
    bytes
}

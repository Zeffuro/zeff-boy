#![allow(dead_code)]

use std::ops::Range;
use zeff_emu_common::save_state::StateReader;
use zeff_md_core::constants::MAX_STATE_BYTES;

pub struct Sections {
    pub version: u32,
    pub save: Range<usize>,
}

fn section(reader: &mut StateReader<'_>) -> Range<usize> {
    let start = reader.position() + 4;
    let length = reader.read_slice(MAX_STATE_BYTES).unwrap().len();
    start..start + length
}

pub fn sections(bytes: &[u8]) -> Sections {
    assert_eq!(&bytes[..8], b"ZMDRAW01");
    let mut reader = StateReader::new(bytes);
    reader.set_position(8);
    let version = reader.read_u32().unwrap();
    assert_eq!(version, 1);
    reader.set_position(52);
    for _ in 0..3 {
        section(&mut reader);
    }
    reader.read_bool().unwrap();
    let save = section(&mut reader);
    Sections { version, save }
}

pub fn save_range(bytes: &[u8]) -> Range<usize> {
    sections(bytes).save
}

pub fn serial_range(bytes: &[u8], cells: usize) -> Range<usize> {
    let save = save_range(bytes);
    assert_eq!(&bytes[save.start..save.start + 2], &[1, 1]);
    let serial = save.start + 2 + cells..save.end - 1;
    assert_eq!(serial.len(), 23);
    serial
}

pub fn serial_payload_range(bytes: &[u8], cells: usize, length: usize) -> Range<usize> {
    let save = save_range(bytes);
    assert_eq!(bytes[save.start], 1);
    assert!(bytes[save.start + 1] <= 1);
    assert_eq!(bytes[save.end - 1], 0);
    let serial = save.start + 2 + cells..save.end - 1;
    assert_eq!(serial.len(), length);
    serial
}

use std::sync::atomic::AtomicBool;

use super::*;
use crate::huge::catalog::{HugeSong, required_validation_frames};

fn selected_source() -> (Vec<u8>, HugeSong) {
    let raw = crate::huge::discovery::tests::fixture(0x1800, 0xc1fd);
    let bound = discovery::discover(&raw, Default::default(), &AtomicBool::new(false))
        .unwrap()
        .bound
        .remove(0);
    let bytes = isolation::build_from_bound(&raw, &bound, 0, &AtomicBool::new(false))
        .unwrap()
        .bytes;
    let bound = discovery::discover(&bytes, Default::default(), &AtomicBool::new(false))
        .unwrap()
        .bound
        .remove(0);
    let validation_frames = required_validation_frames(bound.song.loop_ticks).unwrap();
    let selected = HugeSong {
        source_sha256: zeff_firmware::sha256_hex(&bytes),
        bound,
        validation_frames,
    };
    (bytes, selected)
}

#[test]
fn synthetic_fixture_translates_every_approved_span_and_round_trips() {
    let (bytes, selected) = selected_source();
    let gbs = build(&bytes, &selected, &AtomicBool::new(false)).unwrap();
    assert_eq!(gbs.bytes.len(), 0x70 + 0x7c00);
    assert_eq!(&gbs.bytes[..6], b"GBS\x01\x01\x01");
    assert_eq!(gbs.translation_delta, 0x200);
    assert_eq!(gbs.descriptor, 0x400);
    assert_eq!(gbs.driver_start, 0x1a00);
    assert_eq!(
        gbs.driver_end,
        0x1a00 + discovery::reference::CODE.len() as u16
    );
    assert_eq!(gbs.bytes[0x0c..0x0e], 0xfff0_u16.to_le_bytes());
    assert_eq!(gbs.bytes[0x0e..0x10], [0, 0]);
    assert_eq!(gbs.init_wrapper.offset, 0x434);
    assert_eq!(gbs.play_wrapper.offset, 0x44f);
    assert_eq!(gbs.init_call, 0x44b);
    assert_eq!(gbs.update_call, 0x44f);
    for (source, target) in gbs.copied_source_spans.iter().zip(&gbs.copied_target_spans) {
        assert_eq!(target.offset, source.offset + 0x200);
        assert_eq!(target.byte_len, source.byte_len);
    }
    let program = &gbs.bytes[HEADER_LEN..];
    assert_eq!(
        &program[gbs.init_wrapper.offset as usize - usize::from(LOAD)
            ..gbs.init_call as usize - usize::from(LOAD)],
        &[
            0xf3, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x26, 0x3e, 0x80, 0xe0, 0x26, 0x3e, 0xff,
            0xe0, 0x25, 0x3e, 0x77, 0xe0, 0x24, 0x21, 0, 4
        ]
    );
    assert_eq!(
        &program[gbs.update_call as usize - usize::from(LOAD)
            ..gbs.update_call as usize - usize::from(LOAD) + 4],
        &[0xcd, 0x69, 0x1e, 0xc9]
    );
    for (source, target) in gbs.copied_source_spans.iter().zip(&gbs.copied_target_spans) {
        if source.byte_len == 192 || source.byte_len == 6 || source.byte_len == 16 {
            let source = source.offset as usize..(source.offset + source.byte_len) as usize;
            let target = target.offset as usize - usize::from(LOAD)
                ..(target.offset + target.byte_len) as usize - usize::from(LOAD);
            assert_eq!(&program[target], &bytes[source]);
        }
    }
    assert_eq!(word(&program[0..], 0x401 - usize::from(LOAD)), 0x420);
    for field in [3, 5, 7, 9, 11, 13, 15, 19] {
        let original = word(&bytes, 0x200 + field);
        let relocated = word(program, 0x400 + field - usize::from(LOAD));
        assert_eq!(relocated, original + 0x200);
    }
    assert_eq!(word(program, 0x430 - usize::from(LOAD)), 0x1200);
    assert_eq!(word(program, 0x432 - usize::from(LOAD)), 0x1300);
    let rip = crate::rips::inspect(
        &gbs.bytes,
        crate::rips::RipFormat::Gbs,
        Default::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(rip.load_address, LOAD);
    assert_eq!(rip.init.cpu_address, gbs.init_wrapper.offset as u16);
    assert_eq!(rip.play.cpu_address, gbs.play_wrapper.offset as u16);
    assert!(rip.warnings.is_empty());
}

#[test]
fn relocated_low_driver_is_regenerated_at_its_new_address() {
    let raw = low_driver_source();
    let bound = discovery::discover(&raw, Default::default(), &AtomicBool::new(false))
        .unwrap()
        .bound
        .remove(0);
    let bytes = isolation::build_from_bound(&raw, &bound, 0, &AtomicBool::new(false))
        .unwrap()
        .bytes;
    let bound = discovery::discover(&bytes, Default::default(), &AtomicBool::new(false))
        .unwrap()
        .bound
        .remove(0);
    let selected = HugeSong {
        source_sha256: zeff_firmware::sha256_hex(&bytes),
        validation_frames: required_validation_frames(bound.song.loop_ticks).unwrap(),
        bound,
    };
    let gbs = build(&bytes, &selected, &AtomicBool::new(false)).unwrap();
    assert_eq!((gbs.translation_delta, gbs.driver_start), (0x200, 0x400));
    let program = &gbs.bytes[HEADER_LEN..];
    for (index, byte) in program
        .iter()
        .take(discovery::reference::CODE.len())
        .enumerate()
    {
        assert_eq!(
            *byte,
            discovery::reference::relocated_byte(index, 0x400, 0xc000)
        );
    }
}

#[test]
fn source_identity_selection_mode_and_cancellation_are_gated() {
    let (bytes, selected) = selected_source();
    let mut changed = bytes.clone();
    changed[0x1000] ^= 1;
    assert!(build(&changed, &selected, &AtomicBool::new(false)).is_err());
    let mut forged = selected.clone();
    forged.bound.evidence.ram_address += 1;
    assert!(build(&bytes, &forged, &AtomicBool::new(false)).is_err());
    let mut cgb = bytes.clone();
    cgb[0x143] = 0x80;
    let mut cgb_selected = selected.clone();
    cgb_selected.source_sha256 = zeff_firmware::sha256_hex(&cgb);
    assert!(build(&cgb, &cgb_selected, &AtomicBool::new(false)).is_err());
    assert!(build(&bytes, &selected, &AtomicBool::new(true)).is_err());
}

#[test]
fn wrapper_gap_must_not_overlap_translated_audio() {
    assert!(
        wrappers(
            &[FileSpan {
                offset: u32::from(LOAD),
                byte_len: u32::from(END - LOAD),
            }],
            27,
            4,
        )
        .is_err()
    );
    assert!(
        relocate_span(
            FileSpan {
                offset: 0x7fff,
                byte_len: 1,
            },
            1,
        )
        .is_err()
    );
}

fn low_driver_source() -> Vec<u8> {
    let base = crate::huge::fixture::song();
    let mut bytes = vec![0; 0x8000];
    bytes[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    bytes[0x1400..0x1530].copy_from_slice(&base[0x200..0x330]);
    bytes[0x2200..0x2340].copy_from_slice(&base[0x1000..0x1140]);
    for field in (1..21).step_by(2) {
        let pointer = word(&bytes, 0x1400 + field);
        if pointer != 0 {
            put_word(&mut bytes, 0x1400 + field, pointer + 0x1200);
        }
    }
    for at in [0x1430, 0x1432] {
        let pointer = word(&bytes, at) + 0x1200;
        put_word(&mut bytes, at, pointer);
    }
    install_driver(&mut bytes, 0x200, 0xc000);
    let update = 0x200 + discovery::reference::UPDATE_OFFSET;
    bytes[0x150..0x157].copy_from_slice(&[0x21, 0, 0x14, 0xcd, 0, 2, 0xc9]);
    bytes[0x40..0x44].copy_from_slice(&[0xcd, update as u8, (update >> 8) as u8, 0xd9]);
    bytes
}

fn install_driver(bytes: &mut [u8], start: usize, ram: u16) {
    for index in 0..discovery::reference::CODE.len() {
        bytes[start + index] = discovery::reference::relocated_byte(index, start as u16, ram);
    }
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn put_word(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

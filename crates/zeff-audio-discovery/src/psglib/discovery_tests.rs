use std::sync::atomic::AtomicBool;

use super::discovery::{Budget, fingerprint, matches_driver};
use super::{HeldKind, discover};
use crate::{ScanLimits, ScanStop};

mod multiple;
mod tables;

fn limits() -> ScanLimits {
    ScanLimits {
        max_work: 4_000_000,
        max_candidates: 4,
    }
}

fn word(image: &mut [u8], at: usize, value: u16) {
    image[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn image(start: usize, base: u16, wrapper: bool) -> Vec<u8> {
    let mut image = vec![0xc9; 0x8000];
    image[start..start + fingerprint::REFERENCE.len()].copy_from_slice(&fingerprint::REFERENCE);
    for (&at, delta) in fingerprint::CODE_OPERANDS
        .iter()
        .map(|at| (at, start as i32 - 0xeb))
        .chain(
            fingerprint::RAM_OPERANDS
                .iter()
                .map(|at| (at, i32::from(base) - 0xc000)),
        )
    {
        let value = u16::from_le_bytes([image[start + at], image[start + at + 1]]);
        word(&mut image, start + at, (i32::from(value) + delta) as u16);
    }
    let play = (start + 0x32) as u16;
    image[..3].copy_from_slice(&[0xc3, 0x40, 0]);
    image[0x38] = 0xcd;
    word(&mut image, 0x39, (start + 0x21f) as u16);
    image[0x40..0x46].copy_from_slice(&[0x21, 0, 0x20, 0xcd, 0, 0]);
    word(&mut image, 0x44, if wrapper { 0x100 } else { play });
    if wrapper {
        image[0x100..0x116].copy_from_slice(&[
            0xcd, 0, 0, 0xaf, 0x32, 0, 0, 0xfd, 0x21, 2, 0, 0xfd, 0x39, 0xfd, 0x7e, 0, 0x32, 0, 0,
            0xe1, 0x33, 0xe9,
        ]);
        word(&mut image, 0x101, play);
        word(&mut image, 0x105, base + 8);
        word(&mut image, 0x111, base + 9);
    }
    image[0x2000..0x2003].copy_from_slice(&[0x90, 0x38, 0]);
    image
}

#[test]
fn full_driver_and_literal_calls_bind_with_code_and_ram_relocation() {
    for (start, base, wrapper) in [(0x500, 0xc100, false), (0xa33, 0xcdab, true)] {
        let mut bytes = image(start, base, wrapper);
        bytes[0x6000] = 0x55;
        let report = discover(&bytes, limits(), &AtomicBool::new(false)).unwrap();
        assert_eq!(report.bound.len(), 1);
        assert!(report.held.is_empty());
        let song = &report.bound[0];
        assert_eq!((song.offset, song.frames, song.write_count), (0x2000, 2, 5));
        assert_eq!(song.call_sites[0].offset, 0x40);
        assert_eq!(song.call_roots, [1]);
        assert_eq!(song.evidence.frame_call_sites[0].offset, 0x38);
        assert_eq!(song.evidence.frame_call_roots, [2]);
        assert_eq!(song.evidence.code_delta, start as i32 - 0xeb);
        assert_eq!(song.evidence.ram_delta, i32::from(base) - 0xc000);
        assert_eq!(song.evidence.psg_play_loops.is_some(), wrapper);
    }
}

#[test]
fn every_executable_byte_and_relocation_operand_is_required() {
    let cancel = AtomicBool::new(false);
    let bytes = image(0x500, 0xc100, false);
    for index in 0..fingerprint::REFERENCE.len() {
        let mut bad = bytes.clone();
        bad[0x500 + index] ^= 1;
        assert!(
            !matches_driver(
                &bad,
                0x500,
                0xc100,
                &mut Budget::new(limits(), &cancel).unwrap()
            )
            .unwrap(),
            "byte {index:x}"
        );
    }
    let mut offsets = fingerprint::CODE_OPERANDS.to_vec();
    offsets.extend(fingerprint::RAM_OPERANDS);
    offsets.sort_unstable();
    assert_eq!(offsets.len(), 145);
    assert!(offsets.windows(2).all(|pair| pair[1] > pair[0] + 1));
}

#[test]
fn dead_code_and_literal_call_bytes_do_not_bind() {
    let cancel = AtomicBool::new(false);
    for remove_at in [0x38, 0x40] {
        let mut bytes = image(0x500, 0xc100, false);
        bytes[remove_at] = 0xc9;
        bytes[0x6000..0x6006].copy_from_slice(&[0x21, 0, 0x20, 0xcd, 0x32, 5]);
        let report = discover(&bytes, limits(), &cancel).unwrap();
        assert!(report.bound.is_empty());
        assert_eq!(report.held[0].kind, HeldKind::FingerprintOnly);
    }
    let mut bytes = image(0x500, 0xc100, false);
    bytes[0x40..0x47].copy_from_slice(&[0x21, 0, 0x20, 0x23, 0xcd, 0x32, 5]);
    assert!(
        discover(&bytes, limits(), &cancel)
            .unwrap()
            .bound
            .is_empty()
    );
    let mut bytes = image(0x500, 0xc100, true);
    bytes[0x115] = 0;
    assert!(
        discover(&bytes, limits(), &cancel)
            .unwrap()
            .bound
            .is_empty()
    );
}

#[test]
fn aliases_and_stream_mutations_preserve_only_valid_unique_sequences() {
    let mut bytes = image(0x500, 0xc100, false);
    let call = bytes[0x40..0x46].to_vec();
    bytes[0x46..0x4c].copy_from_slice(&call);
    bytes[0x2000] = 0x93;
    let report = discover(&bytes, limits(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.bound.len(), 1);
    assert_eq!(report.bound[0].call_sites.len(), 2);
    assert_eq!(report.bound[0].call_roots, [1, 1]);
    assert_eq!(report.candidate_count, 2);
    for (pointer, stream, kind) in [
        (0xffff, 0x90, HeldKind::InvalidPointer),
        (0x2000, 2, HeldKind::InvalidStream),
    ] {
        let mut bytes = image(0x500, 0xc100, false);
        word(&mut bytes, 0x41, pointer);
        bytes[0x2000] = stream;
        let report = discover(&bytes, limits(), &AtomicBool::new(false)).unwrap();
        assert!(report.bound.is_empty());
        assert!(report.held.iter().any(|e| e.kind == kind));
    }
}

#[test]
fn mapping_cancellation_and_limits_do_not_publish_partial_results() {
    let mut banked = image(0x500, 0xc100, false);
    banked.push(0);
    let report = discover(&banked, limits(), &AtomicBool::new(false)).unwrap();
    assert!(report.bound.is_empty());
    assert_eq!(report.held[0].kind, HeldKind::UnsupportedMapping);
    for bytes in [&banked, &image(0x500, 0xc100, false)] {
        assert_eq!(
            discover(bytes, limits(), &AtomicBool::new(true)),
            Err(ScanStop::Cancelled)
        );
    }
    let mut bytes = image(0x500, 0xc100, false);
    let call = bytes[0x40..0x46].to_vec();
    bytes[0x46..0x4c].copy_from_slice(&call);
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_work: 1,
                ..limits()
            },
            &AtomicBool::new(false)
        ),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_candidates: 1,
                ..limits()
            },
            &AtomicBool::new(false)
        ),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(
        discover(
            &bytes,
            ScanLimits {
                max_work: 2_000_000,
                ..limits()
            },
            &AtomicBool::new(false)
        ),
        Err(ScanStop::WorkLimit)
    );
}

use std::sync::atomic::AtomicBool;

use super::*;
use crate::{DetectorState, ScanStatus};

const PROGRAM_LEN: usize = 0x50;

fn read(bytes: &[u8]) -> MusicRip {
    inspect(
        bytes,
        RipFormat::Nsf,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}

fn chunk(bytes: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(id);
    bytes.extend(payload);
}

fn source(flags: u8, chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut bytes = crate::test_support::rips::nsf2_fixture();
    bytes.truncate(0x80 + PROGRAM_LEN);
    bytes[0x7c] = flags;
    for (id, payload) in chunks {
        chunk(&mut bytes, id, payload);
    }
    bytes
}

fn state(bytes: &[u8]) -> ScanStatus {
    scan(
        bytes,
        RipFormat::Nsf,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .status
}

#[test]
fn nsf2_preserves_raw_header_and_metadata_without_file_mapping() {
    let bytes = crate::test_support::rips::nsf2_fixture();
    let rip = read(&bytes);
    assert_eq!(rip.version, Some(2));
    assert_eq!(rip.title, "NSF2 source");
    assert_eq!(
        rip.header,
        FileSpan {
            offset: 0,
            byte_len: 0x80
        }
    );
    assert_eq!(
        rip.opaque_metadata,
        Some(FileSpan {
            offset: 0xd0,
            byte_len: 30,
        })
    );
    assert_eq!(rip.load_address, Some(0x1234));
    assert_eq!(rip.play.map(|entry| entry.cpu_address), Some(0));
    assert_eq!(rip.init.unwrap().initial_source_offset, None);
    assert_eq!(rip.play.and_then(|entry| entry.initial_source_offset), None);
    let RipDetails::Nsf2 {
        raw_flags,
        irq_enabled,
        init_non_returning,
        play_suppressed,
        metadata_required,
        declared_program_bytes,
        header_ntsc_period_us,
        header_pal_period_us,
        header_region_bits,
        header_expansion_bits,
        initial_banks,
        metadata,
        rate_ntsc_period_us,
        rate_pal_period_us,
        rate_dendy_period_us,
    } = rip.details
    else {
        panic!("expected NSF2 details");
    };
    assert_eq!(raw_flags, 0xf0);
    assert!(irq_enabled && init_non_returning && play_suppressed && metadata_required);
    assert_eq!(declared_program_bytes, 0x50);
    assert_eq!(
        (header_ntsc_period_us, header_pal_period_us),
        (16639, 19997)
    );
    assert_eq!((header_region_bits, header_expansion_bits), (3, 0x7f));
    assert_eq!(initial_banks, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(
        metadata.iter().map(|chunk| chunk.id).collect::<Vec<_>>(),
        [*b"RATE", *b"regn", *b"NEND"]
    );
    assert_eq!(
        metadata[0].header,
        FileSpan {
            offset: 0xd0,
            byte_len: 8
        }
    );
    assert_eq!(
        metadata[0].payload,
        FileSpan {
            offset: 0xd8,
            byte_len: 4
        }
    );
    assert_eq!(
        (
            rate_ntsc_period_us,
            rate_pal_period_us,
            rate_dendy_period_us
        ),
        (Some(16639), Some(19997), None)
    );
    assert!(rip.warnings.is_empty(), "{:?}", rip.warnings);
}

#[test]
fn nsf2_feature_bits_are_explicit_and_v1_remains_unchanged() {
    for (flags, expected) in [
        (0x10, (true, false, false, false)),
        (0x20, (false, true, false, false)),
        (0x40, (false, false, true, false)),
        (0x80, (false, false, false, true)),
        (0xf0, (true, true, true, true)),
    ] {
        let rip = read(&source(flags, &[(b"NEND", &[])]));
        assert!(matches!(
            rip.details,
            RipDetails::Nsf2 {
                irq_enabled,
                init_non_returning,
                play_suppressed,
                metadata_required,
                ..
            } if (irq_enabled, init_non_returning, play_suppressed, metadata_required) == expected
        ));
    }
    let mut v1 = crate::test_support::rips::fixture(RipFormat::Nsf);
    v1[0x7c] = 0xf0;
    let rip = read(&v1);
    assert_eq!(rip.version, Some(1));
    assert!(matches!(
        rip.details,
        RipDetails::Nsf {
            nsf2_flags: 0xf0,
            ..
        }
    ));
    assert!(
        rip.warnings
            .contains(&RipWarning::Nsf2FeatureFlags { value: 0xf0 })
    );
}

#[test]
fn nsf2_refuses_unsupported_features_and_malformed_structure() {
    for bytes in [
        source(1, &[(b"NEND", &[])]),
        source(0x80, &[]),
        source(0, &[(b"RATE", &[0, 0]), (b"NEND", &[])]),
        source(0, &[(b"RATE", &[1, 0, 2]), (b"NEND", &[])]),
        source(0, &[(b"RATE", &[1, 0]), (b"RATE", &[1, 0]), (b"NEND", &[])]),
        source(0, &[(b"VRC7", &[]), (b"NEND", &[])]),
        source(0, &[(b"ABCD", &[]), (b"NEND", &[])]),
        source(0, &[(b"NEND", &[1])]),
        source(0, &[(b"NEND", &[]), (b"tlbl", b"trailing\0")]),
    ] {
        assert_eq!(state(&bytes), ScanStatus::Unsupported);
    }
    for id in [b"INFO", b"DATA", b"BANK", b"NSF2"] {
        assert_eq!(
            state(&source(0, &[(id, &[]), (b"NEND", &[])])),
            ScanStatus::Malformed(MalformedInput::InvalidChunkOrder)
        );
    }
    let mut invalid_count = source(0, &[(b"NEND", &[])]);
    invalid_count[6] = 0;
    assert_eq!(
        state(&invalid_count),
        ScanStatus::Malformed(MalformedInput::InvalidSongCount)
    );
    let mut invalid_start = source(0, &[(b"NEND", &[])]);
    invalid_start[7] = invalid_start[6] + 1;
    assert_eq!(
        state(&invalid_start),
        ScanStatus::Malformed(MalformedInput::InvalidFirstSong)
    );
    let mut overflowing = crate::test_support::rips::nsf2_fixture();
    overflowing[0x7d..0x80].copy_from_slice(&[0xff, 0xff, 0x7f]);
    assert_eq!(
        state(&overflowing),
        ScanStatus::Malformed(MalformedInput::ProgramLengthExceedsSource)
    );
    assert_eq!(
        state(b"NESM\x1a\x02"),
        ScanStatus::Malformed(MalformedInput::TruncatedHeader)
    );
    for bytes in [
        source(0, &[(b"RATE", &[1, 0]), (b"NEND", &[])])[..0xd9].to_vec(),
        source(0, &[(b"NEND", &[])])[..0xd7].to_vec(),
    ] {
        assert_eq!(
            state(&bytes),
            ScanStatus::Malformed(MalformedInput::TruncatedChunk)
        );
    }
}

#[test]
fn nsf2_metadata_may_end_at_a_complete_chunk_without_nend() {
    for flags in [0, 0x80] {
        let bytes = source(flags, &[(b"text", b"authored note\0")]);
        let rip = read(&bytes);
        let RipDetails::Nsf2 { metadata, .. } = &rip.details else {
            panic!("expected NSF2 metadata");
        };
        assert_eq!(metadata.len(), 1);
        let last = metadata.last().unwrap();
        assert_eq!(
            last.payload.offset + last.payload.byte_len,
            bytes.len() as u32
        );
        verify(&bytes, &rip, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            state(&bytes[..bytes.len() - 1]),
            ScanStatus::Malformed(MalformedInput::TruncatedChunk)
        );
    }
}

#[test]
fn nsf2_explicit_program_end_and_mandatory_metadata_stay_distinct() {
    for flags in [0, 0x40] {
        let bytes = source(flags, &[]);
        let rip = read(&bytes);
        assert_eq!(rip.program.byte_len, PROGRAM_LEN as u32);
        assert!(rip.opaque_metadata.is_none());
    }
    let mut bytes = source(0x80, &[(b"NEND", &[])]);
    bytes[0x7d..0x80].fill(0);
    assert_eq!(state(&bytes), ScanStatus::Unsupported);
}

#[test]
fn nsf2_work_exhaustion_never_reports_a_complete_inventory() {
    let bytes = crate::test_support::rips::nsf2_fixture();
    let full = scan(
        &bytes,
        RipFormat::Nsf,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    for max_work in 0..full.work_used {
        let report = scan(
            &bytes,
            RipFormat::Nsf,
            ScanLimits {
                max_work,
                ..ScanLimits::default()
            },
            &AtomicBool::new(false),
        );
        assert_eq!(report.status, ScanStatus::Incomplete(ScanStop::WorkLimit));
        assert!(report.music_rips.is_empty());
    }
    let exact = scan(
        &bytes,
        RipFormat::Nsf,
        ScanLimits {
            max_work: full.work_used,
            ..ScanLimits::default()
        },
        &AtomicBool::new(false),
    );
    assert_eq!(exact.status, ScanStatus::Complete);
    assert_eq!(exact.music_rips, full.music_rips);
}

#[test]
fn nsf2_metadata_declarations_and_lengths_remain_raw() {
    let two = read(&source(0, &[(b"RATE", &[1, 0]), (b"NEND", &[])]));
    assert!(matches!(
        two.details,
        RipDetails::Nsf2 {
            rate_ntsc_period_us: Some(1),
            rate_pal_period_us: None,
            rate_dendy_period_us: None,
            ..
        }
    ));
    let six = read(&source(
        0,
        &[(b"RATE", &[1, 0, 2, 0, 3, 0]), (b"NEND", &[])],
    ));
    assert!(matches!(
        six.details,
        RipDetails::Nsf2 {
            rate_ntsc_period_us: Some(1),
            rate_pal_period_us: Some(2),
            rate_dendy_period_us: Some(3),
            ..
        }
    ));
    let mut zero_declared = crate::test_support::rips::nsf2_fixture();
    zero_declared[0x7c] = 0;
    zero_declared[0x7d..0x80].fill(0);
    let rip = read(&zero_declared);
    assert_eq!(rip.program.byte_len as usize, zero_declared.len() - 0x80);
    assert_eq!(rip.opaque_metadata, None);
    assert!(matches!(
        rip.details,
        RipDetails::Nsf2 {
            declared_program_bytes: 0,
            metadata,
            ..
        } if metadata.is_empty()
    ));
}

#[test]
fn nsf2_metadata_bounds_work_and_verify_are_deterministic() {
    let mut empty = vec![0; 0x80];
    empty[..8].copy_from_slice(b"NESM\x1a\x02\x01\x01");
    assert_eq!(state(&empty), ScanStatus::Unsupported);
    let valid = source(
        0,
        &[
            (b"tlbl", b"first\0"),
            (b"tlbl", b"second\0"),
            (b"NEND", &[]),
        ],
    );
    let rip = read(&valid);
    verify(&valid, &rip, &AtomicBool::new(false)).unwrap();
    let mut changed = valid.clone();
    changed[0x80] ^= 1;
    assert!(verify(&changed, &rip, &AtomicBool::new(false)).is_err());
    let mut exact = source(0, &[]);
    for _ in 0..super::nsf2::MAX_CHUNKS - 1 {
        chunk(&mut exact, b"tlbl", &[]);
    }
    chunk(&mut exact, b"NEND", &[]);
    assert_eq!(state(&exact), ScanStatus::Complete);
    let mut capped = source(0, &[]);
    for _ in 0..=super::nsf2::MAX_CHUNKS {
        chunk(&mut capped, b"tlbl", &[]);
    }
    assert_eq!(
        state(&capped),
        ScanStatus::Incomplete(ScanStop::InventoryLimit)
    );
    let cancelled = scan(
        &valid,
        RipFormat::Nsf,
        ScanLimits::default(),
        &AtomicBool::new(true),
    );
    assert_eq!(
        cancelled.status,
        ScanStatus::Incomplete(ScanStop::Cancelled)
    );
    assert_eq!(
        cancelled.detector_outcomes[0].state,
        DetectorState::NotRun(ScanStop::Cancelled)
    );
}

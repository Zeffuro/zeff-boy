use std::{io::Write, sync::atomic::AtomicBool};

use crate::test_support::vgm::{fixture, put32};
use flate2::{Compression, write::GzEncoder};

use super::*;

fn limits() -> ScanLimits {
    ScanLimits {
        max_work: 20_000,
        max_candidates: 1,
    }
}

#[test]
fn inventories_waits_loop_and_gd3_in_logical_space() {
    let bytes = fixture(true);
    let log = inspect(&bytes, limits(), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(log.title, "Title");
    assert_eq!(log.samples, 1617);
    assert_eq!(log.loop_offset, Some(0x40));
    assert_eq!(log.commands.address_space, VgmAddressSpace::SourceFile);
    assert!(log.gd3.is_some());
    assert!(verify(&bytes, &log, &AtomicBool::new(false)).is_ok());
}

#[test]
fn rejects_truncation_bad_loop_unknown_command_and_gd3_overlap() {
    let bytes = fixture(true);
    for end in 0..bytes.len() {
        assert!(
            inspect(&bytes[..end], limits(), &AtomicBool::new(false))
                .unwrap()
                .is_none()
        );
    }
    let mut bad = fixture(false);
    bad[0x1c..0x20].copy_from_slice(&1u32.to_le_bytes());
    assert!(
        inspect(&bad, limits(), &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    let mut bad = fixture(false);
    bad[0x40] = 0x64;
    assert!(
        inspect(&bad, limits(), &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    let mut bad = fixture(true);
    put32(&mut bad, 0x14, 0x2c);
    assert!(
        inspect(&bad, limits(), &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
}

#[test]
fn retains_declared_tails_and_enforces_budgets_and_cancellation() {
    let mut bytes = fixture(false);
    bytes.extend_from_slice(&[0xd0, 0x0d]);
    let log = inspect(&bytes, limits(), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert!(
        log.warnings
            .iter()
            .any(|warning| matches!(warning, VgmWarning::EofMismatch { .. }))
    );
    assert_eq!(
        inspect(
            &bytes,
            ScanLimits {
                max_work: 1,
                max_candidates: 1
            },
            &AtomicBool::new(false)
        ),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(
        inspect(&bytes, limits(), &AtomicBool::new(true)),
        Err(ScanStop::Cancelled)
    );
}

#[test]
fn decodes_one_strict_gzip_member_and_rejects_crc_bombs_and_members() {
    let source = fixture(false);
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&source).unwrap();
    let gzip = encoder.finish().unwrap();
    assert_eq!(decode(&gzip, &AtomicBool::new(false)).unwrap(), source);
    let mut bad_crc = gzip.clone();
    *bad_crc.last_mut().unwrap() ^= 1;
    assert!(
        inspect(&bad_crc, limits(), &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    let mut members = gzip.clone();
    members.extend_from_slice(&gzip);
    assert!(
        inspect(&members, limits(), &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&vec![0; 2 * 1024 * 1024]).unwrap();
    assert_eq!(
        inspect(
            &encoder.finish().unwrap(),
            limits(),
            &AtomicBool::new(false)
        ),
        Err(ScanStop::MediaLimit)
    );
}

#[test]
fn handles_data_blocks_versioned_reserved_lengths_and_clock_offsets() {
    let mut bytes = fixture(false);
    bytes.truncate(0x40);
    bytes.extend_from_slice(&[
        0x50, 0x90, 0x67, 0x66, 0x80, 11, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 0x62, 0x63,
        0x66,
    ]);
    put32(&mut bytes, 0x0c, 3_579_545);
    put32(&mut bytes, 0x10, 3_579_545);
    let eof = bytes.len() as u32 - 4;
    put32(&mut bytes, 4, eof);
    let log = inspect(&bytes, limits(), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(log.samples, 1617);
    assert_eq!(log.chips.len(), 2);
    assert_eq!(log.command_histogram.get(&0x67), Some(&1));
    let mut second_chip_block = bytes.clone();
    put32(&mut second_chip_block, 0x45, 0x8000_000b);
    assert!(
        inspect(&second_chip_block, limits(), &AtomicBool::new(false))
            .unwrap()
            .is_some()
    );
    let mut old = fixture(false);
    put32(&mut old, 8, 0x150);
    put32(&mut old, 0x34, 0);
    put32(&mut old, 0x1c, 0);
    put32(&mut old, 0x20, 0);
    old[0x40..].copy_from_slice(&[0x40, 0, 0x66]);
    assert!(
        inspect(&old, limits(), &AtomicBool::new(false))
            .unwrap()
            .is_some()
    );
}

#[test]
fn rejects_invalid_gd3_unicode_and_loop_duration_lies() {
    let mut gd3 = fixture(true);
    let start = 0x14 + u32::from_le_bytes(gd3[0x14..0x18].try_into().unwrap()) as usize;
    gd3[start + 12..start + 14].copy_from_slice(&0xd800u16.to_le_bytes());
    assert!(
        inspect(&gd3, limits(), &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    let mut loop_lie = fixture(false);
    put32(&mut loop_lie, 0x20, 1);
    let log = inspect(&loop_lie, limits(), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(log.loop_samples, 1617);
    assert!(
        log.warnings
            .iter()
            .any(|warning| matches!(warning, VgmWarning::DeclaredLoopSamplesMismatch { .. }))
    );
}

#[test]
fn versioned_commands_legacy_clocks_and_wrapping_loop_offsets_are_distinct() {
    let mut old = fixture(false);
    put32(&mut old, 8, 0x100);
    put32(&mut old, 0x10, 7_670_454);
    put32(&mut old, 0x1c, 0);
    put32(&mut old, 0x20, 0);
    old.truncate(0x40);
    old.extend_from_slice(&[0x52, 0x22, 0, 0x62, 0x66]);
    let eof = old.len() as u32 - 4;
    put32(&mut old, 4, eof);
    let log = inspect(&old, limits(), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(
        log.chips,
        [VgmChip {
            name: "ym2612",
            raw_clock: 7_670_454
        }]
    );
    old[0x40] = 0x90;
    assert!(
        inspect(&old, limits(), &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    let mut wrapped = fixture(false);
    put32(&mut wrapped, 0x1c, 0xffff_fff0);
    assert!(
        inspect(&wrapped, limits(), &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    for version in [0x160, 0x161] {
        let mut reserved = fixture(false);
        put32(&mut reserved, 8, version);
        put32(&mut reserved, 0x1c, 0);
        put32(&mut reserved, 0x20, 0);
        reserved.truncate(0x40);
        reserved.extend_from_slice(if version == 0x160 {
            &[0x40, 0, 0x66][..]
        } else {
            &[0x40, 0, 0, 0x66][..]
        });
        let eof = reserved.len() as u32 - 4;
        put32(&mut reserved, 4, eof);
        assert_eq!(
            inspect(&reserved, limits(), &AtomicBool::new(false))
                .unwrap()
                .unwrap()
                .command_count,
            2
        );
    }
}

#[test]
fn header_fields_that_overlap_commands_are_zero_filled_and_version_gated() {
    let mut bytes = fixture(false);
    bytes.resize(0xe4, 0);
    bytes[0x40..0x43].fill(0);
    put32(&mut bytes, 0x34, 0xe4 - 0x34);
    put32(&mut bytes, 0x1c, 0);
    put32(&mut bytes, 0x20, 0);
    for (_, offset, _) in CLOCKS {
        put32(&mut bytes, *offset, *offset as u32);
    }
    bytes.push(0x66);
    let eof = bytes.len() as u32 - 4;
    put32(&mut bytes, 4, eof);
    let log = inspect(&bytes, limits(), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(log.chips.len(), CLOCKS.len());
    for (chip, (name, offset, _)) in log.chips.iter().zip(CLOCKS) {
        assert_eq!(chip.name, *name);
        assert_eq!(chip.raw_clock, *offset as u32);
    }
    put32(&mut bytes, 8, 0x151);
    assert_eq!(
        inspect(&bytes, limits(), &AtomicBool::new(false))
            .unwrap()
            .unwrap()
            .chips
            .len(),
        19
    );
    let mut partial = fixture(false);
    partial.truncate(0x40);
    partial.extend_from_slice(&[0x12, 0x66]);
    put32(&mut partial, 0x34, 0x41 - 0x34);
    put32(&mut partial, 0x1c, 0);
    put32(&mut partial, 0x20, 0);
    let eof = partial.len() as u32 - 4;
    put32(&mut partial, 4, eof);
    let log = inspect(&partial, limits(), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(
        log.chips,
        [VgmChip {
            name: "rf5c68",
            raw_clock: 0x12
        }]
    );
}

#[test]
fn block_ranges_and_failed_scan_work_are_validated_without_allocating_chip_memory() {
    for (kind, payload, valid) in [
        (0x00, vec![1, 2, 3], true),
        (0x80, vec![1, 2, 3], false),
        (0x80, vec![1, 0, 0, 0, 0, 0, 0, 0, 7], true),
        (0x80, vec![1, 0, 0, 0, 1, 0, 0, 0, 7], false),
        (0xc0, vec![1], false),
        (0xe0, vec![1, 2, 3], false),
    ] {
        let mut bytes = fixture(false);
        bytes.truncate(0x40);
        put32(&mut bytes, 0x1c, 0);
        put32(&mut bytes, 0x20, 0);
        bytes.extend_from_slice(&[0x67, 0x66, kind]);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes.push(0x66);
        let eof = bytes.len() as u32 - 4;
        put32(&mut bytes, 4, eof);
        assert_eq!(
            inspect(&bytes, limits(), &AtomicBool::new(false))
                .unwrap()
                .is_some(),
            valid
        );
        if kind == 0 {
            bytes[0x46] |= 0x80;
            assert!(
                inspect(&bytes, limits(), &AtomicBool::new(false))
                    .unwrap()
                    .is_none()
            );
        }
    }
    let bytes = fixture(true);
    let report = scan(
        &bytes,
        ScanLimits {
            max_work: 4,
            ..limits()
        },
        &AtomicBool::new(false),
    );
    assert_eq!(report.status, ScanStatus::Incomplete(ScanStop::WorkLimit));
    assert_eq!(report.work_used, 4);
    let report = scan(
        &bytes,
        ScanLimits {
            max_candidates: 0,
            ..limits()
        },
        &AtomicBool::new(false),
    );
    assert_eq!(
        report.status,
        ScanStatus::Incomplete(ScanStop::CandidateLimit)
    );
    assert!(report.work_used > 3);
}

#[test]
fn later_data_block_layouts_are_opaque_in_older_versions() {
    for (version, kind, accepted) in [
        (0x150, 0x80, true),
        (0x151, 0x80, false),
        (0x151, 0x40, true),
        (0x160, 0x40, false),
    ] {
        let command = [0x67, 0x66, kind, 1, 0, 0, 0, 0];
        assert_eq!(
            structure::command(0x67, &command, 0, version).is_some(),
            accepted
        );
    }
    for (opcode, minimum, bytes) in [
        (0x31, 0x171, vec![0x31, 0]),
        (0xc4, 0x161, vec![0xc4, 0, 0, 0]),
    ] {
        assert!(structure::command(opcode, &bytes, 0, minimum).is_some());
        assert!(structure::command(opcode, &bytes, 0, 0x151).is_none());
    }
}

#[test]
fn clock_flags_without_a_frequency_do_not_invent_a_chip() {
    for flags in [0x4000_0000, 0x8000_0000, 0xc000_0000] {
        let mut bytes = fixture(false);
        put32(&mut bytes, 0x0c, flags);
        put32(&mut bytes, 0x10, flags);
        let log = inspect(&bytes, limits(), &AtomicBool::new(false))
            .unwrap()
            .unwrap();
        assert!(log.chips.is_empty());
    }
}

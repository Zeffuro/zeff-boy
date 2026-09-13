use super::*;

use crate::test_support::rips::fixture;

fn read(bytes: &[u8], format: RipFormat) -> MusicRip {
    inspect(
        bytes,
        format,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}

#[test]
fn import_keeps_complete_identity_and_one_based_declared_song_numbers() {
    for format in [RipFormat::Gbs, RipFormat::Nsf] {
        let bytes = fixture(format);
        let rip = read(&bytes, format);
        assert_eq!((rip.song_count, rip.first_song), (3, 2));
        assert_eq!(rip.title, "Test source");
        assert_eq!(
            rip.source,
            FileSpan {
                offset: 0,
                byte_len: bytes.len() as u32
            }
        );
        assert_eq!(rip.sha256, zeff_firmware::sha256_hex(&bytes));
        assert_eq!(rip.program.offset, rip.header.byte_len);
        assert_eq!(rip.program.byte_len, 0x50);
        assert_eq!(rip.init.initial_source_offset, Some(rip.program.offset));
        assert_eq!(rip.play.initial_source_offset, Some(rip.program.offset + 1));
        assert!(rip.opaque_metadata.is_none());
        assert!(rip.warnings.is_empty(), "{:?}", rip.warnings);
    }
}

#[test]
fn short_headers_versions_invalid_song_numbers_and_addresses_fail_closed() {
    for format in [RipFormat::Gbs, RipFormat::Nsf] {
        let bytes = fixture(format);
        let header = if format == RipFormat::Gbs { 0x70 } else { 0x80 };
        let version = if format == RipFormat::Gbs { 3 } else { 5 };
        let address = if format == RipFormat::Gbs { 6 } else { 8 };
        for len in 0..=header {
            assert!(
                inspect(
                    &bytes[..len],
                    format,
                    ScanLimits::default(),
                    &AtomicBool::new(false)
                )
                .unwrap()
                .is_none()
            );
        }
        for (at, value) in [
            (0, 0),
            (version, 2),
            (version + 1, 0),
            (version + 2, 0),
            (version + 2, 4),
            (address + 1, 0),
        ] {
            let mut malformed = bytes.clone();
            malformed[at] = value;
            assert!(
                inspect(
                    &malformed,
                    format,
                    ScanLimits::default(),
                    &AtomicBool::new(false)
                )
                .unwrap()
                .is_none()
            );
        }
        let other = if format == RipFormat::Gbs {
            RipFormat::Nsf
        } else {
            RipFormat::Gbs
        };
        assert!(
            inspect(
                &bytes,
                other,
                ScanLimits::default(),
                &AtomicBool::new(false)
            )
            .unwrap()
            .is_none()
        );
    }
}

#[test]
fn gbs_load_page_and_partial_final_page_are_not_file_relative_banks() {
    let mut bytes = fixture(RipFormat::Gbs);
    bytes.resize(0x70 + 0x4081, 0x35);
    bytes[6..8].copy_from_slice(&0x3f80u16.to_le_bytes());
    bytes[8..10].copy_from_slice(&0x4000u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&0x4001u16.to_le_bytes());
    let rip = read(&bytes, RipFormat::Gbs);
    assert_eq!(rip.init.initial_source_offset, Some(0xf0));
    assert!(matches!(
        rip.details,
        RipDetails::Gbs {
            logical_page_count: 3,
            leading_padding: 0x3f80,
            ..
        }
    ));
    bytes[6..8].copy_from_slice(&0x4000u16.to_le_bytes());
    bytes.truncate(0x71);
    let rip = read(&bytes, RipFormat::Gbs);
    assert!(matches!(
        rip.details,
        RipDetails::Gbs {
            logical_page_count: 2,
            leading_padding: 0x4000,
            ..
        }
    ));
    assert_eq!(rip.init.initial_source_offset, Some(0x70));
    assert_eq!(rip.play.initial_source_offset, None);
}

#[test]
fn gbs_initial_timer_rates_retain_clock_and_extension_uncertainty() {
    let mut bytes = fixture(RipFormat::Gbs);
    bytes[0x0e] = 255;
    for (control, numerator, denominator) in [
        (0, 4_194_304, 70_224),
        (0x80, 4_194_304, 70_224),
        (4, 4096, 1),
        (5, 262_144, 1),
        (6, 65_536, 1),
        (7, 16_384, 1),
        (0x85, 524_288, 1),
    ] {
        bytes[0x0f] = control;
        assert!(
            matches!(read(&bytes, RipFormat::Gbs).details, RipDetails::Gbs { initial_play_rate_hz: Some(rate), .. } if rate == Rate { numerator, denominator })
        );
    }
    bytes[0x0e] = 0;
    bytes[0x0f] = 5;
    assert!(matches!(
        read(&bytes, RipFormat::Gbs).details,
        RipDetails::Gbs {
            initial_play_rate_hz: Some(Rate {
                numerator: 262_144,
                denominator: 256
            }),
            ..
        }
    ));
    for control in [0x0c, 0x44, 0x84 | 0x38] {
        bytes[0x0f] = control;
        assert!(matches!(
            read(&bytes, RipFormat::Gbs).details,
            RipDetails::Gbs {
                initial_play_rate_hz: None,
                ..
            }
        ));
    }
    bytes[0x0f] = 0x44;
    bytes.truncate(0x71);
    let rip = read(&bytes, RipFormat::Gbs);
    assert!(rip.warnings.contains(&RipWarning::CustomInterruptVectors));
    assert!(rip.warnings.contains(&RipWarning::UnbackedInterruptVectors));
}

#[test]
fn nsf_declared_program_length_separates_opaque_appended_metadata() {
    let mut bytes = fixture(RipFormat::Nsf);
    bytes[0x7d] = 2;
    bytes[0x82..].fill(0xa5);
    let rip = read(&bytes, RipFormat::Nsf);
    assert_eq!(
        rip.program,
        FileSpan {
            offset: 0x80,
            byte_len: 2
        }
    );
    assert_eq!(
        rip.opaque_metadata,
        Some(FileSpan {
            offset: 0x82,
            byte_len: 0x4e
        })
    );
    assert_eq!(rip.source.byte_len as usize, bytes.len());
    bytes[0x7d] = 1;
    assert_eq!(
        read(&bytes, RipFormat::Nsf).play.initial_source_offset,
        None
    );
    bytes[0x7d] = 0x50;
    assert!(read(&bytes, RipFormat::Nsf).opaque_metadata.is_none());
    bytes[0x7d] = 0x51;
    assert!(
        inspect(
            &bytes,
            RipFormat::Nsf,
            ScanLimits::default(),
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_none()
    );
    bytes[0x7d..0x80].fill(0xff);
    assert!(
        inspect(
            &bytes,
            RipFormat::Nsf,
            ScanLimits::default(),
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn nsf_initial_banks_resolve_to_source_bytes_including_load_padding() {
    let mut bytes = fixture(RipFormat::Nsf);
    bytes.resize(0x80 + 0x2000, 0x60);
    bytes[8..10].copy_from_slice(&0x8123u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&0x8123u16.to_le_bytes());
    bytes[12..14].copy_from_slice(&0xa122u16.to_le_bytes());
    bytes[0x70..0x78].copy_from_slice(&[1, 0, 2, 3, 0, 0, 0, 0]);
    let rip = read(&bytes, RipFormat::Nsf);
    assert_eq!(rip.init.initial_source_offset, Some(0x1080));
    assert_eq!(rip.play.initial_source_offset, Some(0x207f));
    assert!(matches!(
        rip.details,
        RipDetails::Nsf {
            bank_count: Some(3),
            leading_padding: 0x123,
            ..
        }
    ));
    assert!(
        rip.warnings
            .contains(&RipWarning::InitialBankBeyondProgram {
                slot: 3,
                bank: 3,
                pages: 3
            })
    );
    bytes[10..12].copy_from_slice(&0x9000u16.to_le_bytes());
    assert_eq!(
        read(&bytes, RipFormat::Nsf).init.initial_source_offset,
        None
    );
}

#[test]
fn nsf_fds_low_mappings_retain_the_distinct_initial_bank_slots() {
    let mut bytes = fixture(RipFormat::Nsf);
    bytes.resize(0x80 + 0x8000, 0x60);
    bytes[8..10].copy_from_slice(&0x6000u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&0x6010u16.to_le_bytes());
    bytes[12..14].copy_from_slice(&0x7020u16.to_le_bytes());
    bytes[0x70..0x78].copy_from_slice(&[0, 1, 2, 3, 4, 5, 6, 7]);
    assert!(
        inspect(
            &bytes,
            RipFormat::Nsf,
            ScanLimits::default(),
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_none()
    );
    bytes[0x7b] = 4;
    let rip = read(&bytes, RipFormat::Nsf);
    assert_eq!(rip.init.initial_source_offset, Some(0x6090));
    assert_eq!(rip.play.initial_source_offset, Some(0x70a0));
    assert!(rip.warnings.contains(&RipWarning::FdsMapping));
}

#[test]
fn nsf_raw_region_periods_expansions_and_uninterpreted_flags_are_preserved() {
    let mut bytes = fixture(RipFormat::Nsf);
    for (raw, region) in [
        (0, NsfRegion::Ntsc),
        (1, NsfRegion::Pal),
        (2, NsfRegion::DualNtscPreferred),
        (3, NsfRegion::DualPalPreferred),
    ] {
        bytes[0x7a] = raw;
        let rip = read(&bytes, RipFormat::Nsf);
        assert!(
            matches!(rip.details, RipDetails::Nsf { region: actual, pal_period_us: 0, .. } if actual == region)
        );
        assert_eq!(
            rip.warnings
                .contains(&RipWarning::UnspecifiedPeriod { region: "pal" }),
            raw != 0
        );
    }
    bytes[0x7a] = 0xff;
    bytes[0x7b] = 0xff;
    bytes[0x7c] = 0xf7;
    let rip = read(&bytes, RipFormat::Nsf);
    assert!(
        matches!(rip.details, RipDetails::Nsf { expansion_bits: 255, ref expansion_chips, nsf2_flags: 0xf7, .. } if expansion_chips.len() == 7)
    );
    assert!(rip.warnings.contains(&RipWarning::MultipleExpansionChips));
    assert!(
        rip.warnings
            .contains(&RipWarning::Nsf2FeatureFlags { value: 0xf7 })
    );
    bytes[0x70..0x78].fill(0);
    bytes.resize(0x80 + 0xa001, 0x60);
    assert!(
        read(&bytes, RipFormat::Nsf)
            .warnings
            .contains(&RipWarning::ProgramExceedsLinearMemory { byte_len: 0xa001 })
    );
}

#[test]
fn text_display_preserves_legacy_bytes_without_guessing_an_encoding() {
    for format in [RipFormat::Gbs, RipFormat::Nsf] {
        let mut bytes = fixture(format);
        let at = if format == RipFormat::Gbs { 0x10 } else { 0x0e };
        bytes[at..at + 32].fill(b'A');
        let rip = read(&bytes, format);
        assert_eq!(rip.title, "A".repeat(32));
        assert_eq!(
            rip.warnings
                .contains(&RipWarning::UnterminatedText { field: "title" }),
            format == RipFormat::Nsf
        );
        bytes[at..at + 5].copy_from_slice(&[b'A', 0x82, 10, 0, b'B']);
        let rip = read(&bytes, format);
        assert_eq!(rip.title, "A\\x82\\x0A");
        assert!(
            rip.warnings
                .contains(&RipWarning::NonAsciiText { field: "title" })
        );
        assert!(
            rip.warnings
                .contains(&RipWarning::ControlText { field: "title" })
        );
        assert!(
            rip.warnings
                .contains(&RipWarning::NonzeroTextPadding { field: "title" })
        );
        assert_eq!(rip.sha256, zeff_firmware::sha256_hex(&bytes));
        bytes[at..at + 32].fill(0);
        bytes[at..at + 4].copy_from_slice(b"\\x82");
        assert_eq!(read(&bytes, format).title, "\\\\x82");
    }
}

fn assert_scan_outcome(
    bytes: &[u8],
    format: RipFormat,
    status: crate::ScanStatus,
    state: crate::DetectorState,
    work_used: u64,
) {
    let report = scan(
        bytes,
        format,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(report.detector_version, 2);
    assert_eq!(report.status, status);
    assert!(report.music_rips.is_empty());
    assert_eq!(report.song_count(), 0);
    assert_eq!(report.work_used, work_used);
    assert_eq!(report.detector_outcomes.len(), 1);
    assert_eq!(report.detector_outcomes[0].descriptor.semantic_version, 2);
    assert_eq!(report.detector_outcomes[0].state, state);
    assert_eq!(report.detector_outcomes[0].retained_matches, 0);
    assert_eq!(report.detector_outcomes[0].work_used, work_used);
}

#[test]
fn scan_distinguishes_unsupported_malformed_and_incomplete_inputs() {
    use crate::{DetectorState, MalformedInput, ScanStatus};

    for format in [RipFormat::Gbs, RipFormat::Nsf] {
        let bytes = fixture(format);
        let (header_len, version_at, songs_at, address_at) = match format {
            RipFormat::Gbs => (0x70, 3, 4, 6),
            RipFormat::Nsf => (0x80, 5, 6, 8),
        };
        let mut wrong_signature = bytes.clone();
        wrong_signature[0] ^= 0xff;
        assert_scan_outcome(
            &wrong_signature,
            format,
            ScanStatus::Unsupported,
            DetectorState::Unsupported,
            1,
        );
        let mut wrong_version = bytes.clone();
        wrong_version[version_at] = 2;
        assert_scan_outcome(
            &wrong_version,
            format,
            ScanStatus::Unsupported,
            DetectorState::Unsupported,
            1,
        );

        for (malformed, reason, work_used) in [
            (
                bytes[..header_len - 1].to_vec(),
                MalformedInput::TruncatedHeader,
                1,
            ),
            (
                bytes[..header_len].to_vec(),
                MalformedInput::EmptyProgram,
                1,
            ),
            (
                {
                    let mut value = bytes.clone();
                    value[songs_at] = 0;
                    value
                },
                MalformedInput::InvalidSongCount,
                1,
            ),
            (
                {
                    let mut value = bytes.clone();
                    value[songs_at + 1] = value[songs_at] + 1;
                    value
                },
                MalformedInput::InvalidFirstSong,
                1,
            ),
            (
                {
                    let mut value = bytes.clone();
                    value[address_at + 1] = 0;
                    value
                },
                MalformedInput::InvalidAddress,
                2,
            ),
        ] {
            assert_scan_outcome(
                &malformed,
                format,
                ScanStatus::Malformed(reason),
                DetectorState::Malformed(reason),
                work_used,
            );
        }

        let budget_limited = scan(
            &bytes,
            format,
            ScanLimits {
                max_work: 0,
                ..ScanLimits::default()
            },
            &AtomicBool::new(false),
        );
        assert_eq!(
            budget_limited.status,
            ScanStatus::Incomplete(ScanStop::WorkLimit)
        );
        assert_eq!(
            budget_limited.detector_outcomes[0].state,
            DetectorState::Incomplete(ScanStop::WorkLimit)
        );
        assert_eq!(budget_limited.work_used, 0);

        let cancelled = scan(
            &bytes,
            format,
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
        assert_eq!(cancelled.work_used, 0);
    }

    let mut nsf_overrun = fixture(RipFormat::Nsf);
    nsf_overrun[0x7d..0x80].copy_from_slice(&[0x51, 0, 0]);
    assert_scan_outcome(
        &nsf_overrun,
        RipFormat::Nsf,
        ScanStatus::Malformed(MalformedInput::ProgramLengthExceedsSource),
        DetectorState::Malformed(MalformedInput::ProgramLengthExceedsSource),
        2,
    );
    assert_eq!(
        serde_json::to_value(ScanStatus::Malformed(MalformedInput::InvalidAddress)).unwrap(),
        serde_json::json!({ "kind": "malformed", "reason": "invalid_address" })
    );
}

#[test]
fn bounded_work_cancellation_and_forged_inventories_are_rejected() {
    let bytes = fixture(RipFormat::Nsf);
    for max_work in 0..3 {
        assert_eq!(
            inspect(
                &bytes,
                RipFormat::Nsf,
                ScanLimits {
                    max_work,
                    max_candidates: 1
                },
                &AtomicBool::new(false)
            ),
            Err(ScanStop::WorkLimit)
        );
    }
    assert_eq!(
        inspect(
            &bytes,
            RipFormat::Nsf,
            ScanLimits::default(),
            &AtomicBool::new(true)
        ),
        Err(ScanStop::Cancelled)
    );
    assert_eq!(
        inspect(
            &bytes,
            RipFormat::Nsf,
            ScanLimits {
                max_work: MAX_SCAN_WORK + 1,
                max_candidates: 1
            },
            &AtomicBool::new(false)
        ),
        Err(ScanStop::InvalidLimits)
    );
    let rip = read(&bytes, RipFormat::Nsf);
    verify(&bytes, &rip, &AtomicBool::new(false)).unwrap();
    let mut forged = rip.clone();
    forged.program.byte_len -= 1;
    assert!(verify(&bytes, &forged, &AtomicBool::new(false)).is_err());
    let mut changed = bytes;
    *changed.last_mut().unwrap() ^= 1;
    assert!(verify(&changed, &rip, &AtomicBool::new(false)).is_err());
}

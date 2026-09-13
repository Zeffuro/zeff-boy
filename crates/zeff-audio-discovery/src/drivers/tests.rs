use super::*;

#[test]
fn discovery_preflight_reports_cancellation_invalid_limits_and_unsupported_systems() {
    let bytes = vec![0; 0x150];
    let defaults = ScanLimits::default();
    for (system, limits, cancelled, expected) in [
        (
            System::Gb,
            defaults,
            true,
            ScanStatus::Incomplete(ScanStop::Cancelled),
        ),
        (
            System::Gb,
            ScanLimits {
                max_work: crate::MAX_SCAN_WORK + 1,
                ..defaults
            },
            false,
            ScanStatus::Incomplete(ScanStop::InvalidLimits),
        ),
        (
            System::Gb,
            ScanLimits {
                max_candidates: crate::MAX_CANDIDATES + 1,
                ..defaults
            },
            false,
            ScanStatus::Incomplete(ScanStop::InvalidLimits),
        ),
        (System::Nes, defaults, false, ScanStatus::Unsupported),
    ] {
        let report = scan(system, &bytes, limits, &AtomicBool::new(cancelled));
        assert_eq!(report.status, expected);
        assert_eq!(report.work_used, 0);
        assert!(report.findings.is_empty());
        if matches!(expected, ScanStatus::Incomplete(_)) {
            assert!(report.media.sha256.is_none());
        }
    }
}

#[test]
fn execution_fixture_does_not_count_as_a_retail_driver_fingerprint() {
    let bytes = crate::gb_native::cgb_fixture_rom();
    let cancel = AtomicBool::new(false);
    let report = scan(System::Gb, &bytes, ScanLimits::default(), &cancel);
    assert_eq!(report.status, ScanStatus::Complete);
    assert!(report.findings.is_empty());
    assert_eq!(report.media.sha256, Some(zeff_firmware::sha256_hex(&bytes)));
    assert_eq!(
        crate::scan(System::Gb, &bytes, ScanLimits::default(), &cancel)
            .gb_native_songs
            .len(),
        3
    );
}

#[test]
fn oversized_media_is_rejected_without_hashing() {
    let bytes = vec![0; crate::MAX_ROM_BYTES + 1];
    let report = scan(
        System::Gb,
        &bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(report.status, ScanStatus::Incomplete(ScanStop::MediaLimit));
    assert!(report.media.sha256.is_none());
    assert!(report.findings.is_empty());
}

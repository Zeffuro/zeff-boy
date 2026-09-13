use std::sync::atomic::AtomicBool;

use zeff_emu_common::system::System;

use crate::{
    DetectorState, ScanLimits, ScanStatus, ScanStop, scan, scan_standalone_tracker,
    test_support::{collection, fixture},
    tracker::EmbeddedFormat,
};

fn outcome<'a>(report: &'a crate::ScanReport, id: &str) -> &'a crate::DetectorOutcome {
    report
        .detector_outcomes
        .iter()
        .find(|outcome| outcome.descriptor.id == id)
        .expect("every applicable detector has an outcome")
}

#[test]
fn preflight_marks_every_applicable_detector_not_run_without_work() {
    let report = scan(
        System::Gba,
        &[0; 256],
        ScanLimits::default(),
        &AtomicBool::new(true),
    );

    assert_eq!(report.status, ScanStatus::Incomplete(ScanStop::Cancelled));
    assert_eq!(report.work_used, 0);
    assert_eq!(report.detector_outcomes.len(), 15);
    assert!(report.detector_outcomes.iter().all(|outcome| {
        outcome.state == DetectorState::NotRun(ScanStop::Cancelled)
            && outcome.work_used == 0
            && outcome.retained_matches == 0
    }));
}

#[test]
fn sequential_stop_retains_completed_tracker_and_marks_later_mp2k_not_run() {
    let report = scan(
        System::Gba,
        &[0; 256],
        ScanLimits {
            max_work: 1,
            ..ScanLimits::default()
        },
        &AtomicBool::new(false),
    );

    assert_eq!(report.status, ScanStatus::Incomplete(ScanStop::WorkLimit));
    assert_eq!(
        outcome(&report, "tracker-structure").state,
        DetectorState::Complete
    );
    assert_eq!(
        outcome(&report, "gax3-structure").state,
        DetectorState::Incomplete(ScanStop::WorkLimit)
    );
    assert_eq!(
        outcome(&report, "mp2k-sequence").state,
        DetectorState::NotRun(ScanStop::WorkLimit)
    );
    assert_eq!(
        report
            .detector_outcomes
            .iter()
            .map(|outcome| outcome.work_used)
            .sum::<u64>(),
        report.work_used
    );
}

#[test]
fn composite_mp2k_can_be_partial_after_gax_completed() {
    let mut bytes = fixture();
    collection(&mut bytes, 0x800);
    let report = scan(
        System::Gba,
        &bytes,
        ScanLimits {
            max_candidates: 1,
            ..ScanLimits::default()
        },
        &AtomicBool::new(false),
    );

    assert_eq!(
        report.status,
        ScanStatus::Incomplete(ScanStop::CandidateLimit)
    );
    assert_eq!(
        outcome(&report, "gax3-structure").state,
        DetectorState::Complete
    );
    let mp2k = outcome(&report, "mp2k-sequence");
    assert_eq!(
        mp2k.state,
        DetectorState::Incomplete(ScanStop::CandidateLimit)
    );
    assert_eq!(mp2k.retained_matches as usize, report.candidates.len());
    assert_eq!(
        report
            .detector_outcomes
            .iter()
            .map(|outcome| outcome.retained_matches as usize)
            .sum::<usize>(),
        report.song_count()
    );
}

#[test]
fn completed_empty_and_standalone_rejection_have_explicit_states() {
    let empty = scan(
        System::Gb,
        &[],
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(empty.status, ScanStatus::Complete);
    assert!(
        empty
            .detector_outcomes
            .iter()
            .all(|outcome| outcome.state == DetectorState::Complete)
    );
    assert!(
        empty
            .detector_outcomes
            .iter()
            .all(|outcome| outcome.retained_matches == 0)
    );

    let rejected = scan_standalone_tracker(
        &[],
        EmbeddedFormat::Xm,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(rejected.status, ScanStatus::Unsupported);
    assert_eq!(
        outcome(&rejected, "tracker-structure").state,
        DetectorState::Unsupported
    );
}

#[test]
fn zero_limits_distinguish_empty_completion_from_an_attempted_match() {
    let limits = ScanLimits {
        max_work: 0,
        max_candidates: 0,
    };
    let cancel = AtomicBool::new(false);
    let empty = scan(System::Pce, &[], limits, &cancel);
    assert_eq!(empty.status, ScanStatus::Complete);
    assert!(
        empty
            .detector_outcomes
            .iter()
            .all(|item| item.state == DetectorState::Complete && item.work_used == 0)
    );
    let native = scan(System::Gb, &[], limits, &cancel);
    assert_eq!(native.status, ScanStatus::Incomplete(ScanStop::WorkLimit));
    assert_eq!(
        outcome(&native, "tracker-structure").state,
        DetectorState::Complete
    );
    assert_eq!(
        outcome(&native, "gb-banked-driver").state,
        DetectorState::Incomplete(ScanStop::WorkLimit)
    );
    assert_eq!(native.work_used, 0);
    let attempted = scan(System::Gba, &fixture(), limits, &cancel);
    assert_eq!(
        attempted.status,
        ScanStatus::Incomplete(ScanStop::WorkLimit)
    );
    assert_eq!(
        outcome(&attempted, "tracker-structure").state,
        DetectorState::Incomplete(ScanStop::WorkLimit)
    );
    assert_eq!(
        outcome(&attempted, "gax3-structure").state,
        DetectorState::NotRun(ScanStop::WorkLimit)
    );
    let format = crate::rips::RipFormat::Gbs;
    let bytes = crate::test_support::rips::fixture(format);
    let limited = crate::rips::scan(
        &bytes,
        format,
        ScanLimits {
            max_candidates: 0,
            ..ScanLimits::default()
        },
        &cancel,
    );
    assert_eq!(
        limited.status,
        ScanStatus::Incomplete(ScanStop::CandidateLimit)
    );
    assert_eq!(limited.work_used, 3);
    assert_eq!(
        limited.detector_outcomes[0].state,
        DetectorState::Incomplete(ScanStop::CandidateLimit)
    );
    assert_eq!(limited.detector_outcomes[0].retained_matches, 0);
}

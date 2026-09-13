use super::{
    Budget, DetectorState, MAX_ROM_BYTES, MediaIdentity, ScanLimits, ScanReport, ScanStatus,
    ScanStop, detectors, tracker,
};
use std::sync::atomic::{AtomicBool, Ordering};
pub fn scan_standalone_tracker(
    bytes: &[u8],
    format: tracker::EmbeddedFormat,
    limits: ScanLimits,
    cancel: &AtomicBool,
) -> ScanReport {
    let cancelled = cancel.load(Ordering::Relaxed);
    let mut report = ScanReport::new(
        "standalone-tracker-structural",
        3,
        detectors::TRACKER,
        &[
            "Only a validated XM 1.04, 31-instrument MOD, S3M or IT beginning at byte offset zero is accepted as a standalone module.",
            "S3M OPL instruments and packed samples, and IT compressed or ADPCM samples, are outside the supported structural profiles.",
            "The module's structural extent ends at its final validated sample. Any remaining source bytes are retained for native export and reported as unsupported trailing bytes.",
            "Standalone module recognition does not attribute the file to a console driver or reproduce tracker playback.",
            "Work units count structural validation steps; the bounded media-identity hash is a separate pass.",
        ],
        MediaIdentity {
            system: "standalone_tracker",
            byte_len: bytes.len() as u64,
            sha256: (bytes.len() <= MAX_ROM_BYTES && !cancelled)
                .then(|| const_hex::encode(zeff_firmware::sha256_bytes(bytes))),
        },
        limits,
    );
    if report.preflight(cancel).is_some() {
        return report;
    }
    let mut budget = Budget {
        cancel,
        remaining: limits.max_work,
    };
    let result = tracker::scan_standalone(bytes, format, &mut budget);
    let state = match result {
        Ok(Some(_)) if limits.max_candidates == 0 => {
            report.status = ScanStatus::Incomplete(ScanStop::CandidateLimit);
            DetectorState::Incomplete(ScanStop::CandidateLimit)
        }
        Ok(Some(module)) => {
            report.tracker_modules.push(module);
            DetectorState::Complete
        }
        Ok(None) => {
            report.status = ScanStatus::Unsupported;
            DetectorState::Unsupported
        }
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.work_used = limits.max_work - budget.remaining;
    report.record_detector(
        detectors::TRACKER[0],
        state,
        report.tracker_modules.len(),
        report.work_used,
    );
    debug_assert_eq!(
        report.detector_outcomes.len(),
        report.applicable_detectors.len()
    );
    report
}

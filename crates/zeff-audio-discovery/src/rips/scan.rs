use std::sync::atomic::{AtomicBool, Ordering};

use super::{RipFormat, RipInspection, inspect_with_budget};
use crate::{
    Budget, DetectorState, MAX_ROM_BYTES, MediaIdentity, ScanLimits, ScanReport, ScanStatus,
    ScanStop, detectors,
};

pub fn scan(
    bytes: &[u8],
    format: RipFormat,
    limits: ScanLimits,
    cancel: &AtomicBool,
) -> ScanReport {
    let descriptors = match format {
        RipFormat::Gbs => detectors::GBS,
        RipFormat::Nsf => detectors::NSF,
    };
    let mut report = ScanReport::new(
        format.detector_id(),
        2,
        descriptors,
        &[
            "Imported GBS v1 and NSF v1 files are structurally inspected and preserved. No guest initialization or playback is performed.",
            "One catalog entry represents the entire rip, with its declared song count and starting song. Header declarations do not identify a native ROM driver or establish individual song data spans.",
            "Initial CPU mappings describe file-backed entry addresses only; bank changes, executable validity, synthesis and runtime behavior are unverified.",
            "Raw timer, region, expansion and reserved fields are retained with explicit limitations. NSF2, NSFe and other rip formats are unsupported.",
            "Work units count header-validation steps; bounded source hashing is a separate pass.",
        ],
        MediaIdentity {
            system: format.system_id(),
            byte_len: bytes.len() as u64,
            sha256: (bytes.len() <= MAX_ROM_BYTES && !cancel.load(Ordering::Relaxed))
                .then(|| zeff_firmware::sha256_hex(bytes)),
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
    let state = match inspect_with_budget(bytes, format, &mut budget) {
        Ok(RipInspection::Match(_)) if limits.max_candidates == 0 => {
            report.status = ScanStatus::Incomplete(ScanStop::CandidateLimit);
            DetectorState::Incomplete(ScanStop::CandidateLimit)
        }
        Ok(RipInspection::Match(rip)) => {
            report.music_rips.push(*rip);
            DetectorState::Complete
        }
        Ok(RipInspection::Unsupported) => {
            report.status = ScanStatus::Unsupported;
            DetectorState::Unsupported
        }
        Ok(RipInspection::Malformed(reason)) => {
            report.status = ScanStatus::Malformed(reason);
            DetectorState::Malformed(reason)
        }
        Err(reason) => {
            report.status = ScanStatus::Incomplete(reason);
            DetectorState::Incomplete(reason)
        }
    };
    report.work_used = limits.max_work - budget.remaining;
    report.record_detector(
        descriptors[0],
        state,
        report.music_rips.len(),
        report.work_used,
    );
    report
}

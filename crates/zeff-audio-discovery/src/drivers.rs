use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use zeff_emu_common::system::System;

use crate::{Budget, MediaIdentity, RomSpan, ScanLimits, ScanStatus, ScanStop};

pub mod gb_fingerprints;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriverReport {
    pub schema: &'static str,
    pub detector_version: u32,
    pub media: MediaIdentity,
    pub limits: ScanLimits,
    pub status: ScanStatus,
    pub work_used: u64,
    pub findings: Vec<DriverFinding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub driver_candidates: Vec<gb_fingerprints::DriverCandidate>,
    pub limitations: &'static [&'static str],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriverFinding {
    pub family: &'static str,
    pub variant: &'static str,
    pub qualification: Qualification,
    pub fingerprint: DriverFingerprint,
    pub parameters: BankedDriverParameters,
    pub rows: Vec<DriverTableRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Qualification {
    Candidate,
    KnownRom {
        profile: &'static str,
        native_playback_selectors: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriverFingerprint {
    pub span: RomSpan,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BankedDriverParameters {
    pub cartridge_type: u8,
    pub init: RomSpan,
    pub selector: RomSpan,
    pub tick: RomSpan,
    pub table: RomSpan,
    pub selector_base: u8,
    pub inspected_rows: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriverTableRow {
    pub raw_selector: u8,
    pub entry: RomSpan,
    pub state: DriverTableRowState,
    pub header: Option<RomSpan>,
    pub channels: Vec<crate::gb_native::GbNativeChannel>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverTableRowState {
    MappedChannelHeader,
    EmptyChannelMask,
    InvalidChannelMask,
    UnmappedHeader,
    UnmappedChannel,
}

pub fn scan(system: System, bytes: &[u8], limits: ScanLimits, cancel: &AtomicBool) -> DriverReport {
    let stop = if cancel.load(Ordering::Relaxed) {
        Some(ScanStop::Cancelled)
    } else if bytes.len() > crate::MAX_ROM_BYTES {
        Some(ScanStop::MediaLimit)
    } else if limits.max_work > crate::MAX_SCAN_WORK
        || limits.max_candidates > crate::MAX_CANDIDATES
    {
        Some(ScanStop::InvalidLimits)
    } else {
        None
    };
    let mut report = DriverReport {
        schema: "zeff-audio-driver-evidence/1",
        detector_version: 2,
        media: MediaIdentity {
            system: system.code(),
            byte_len: bytes.len() as u64,
            sha256: stop.is_none().then(|| zeff_firmware::sha256_hex(bytes)),
        },
        limits,
        status: stop.map_or(ScanStatus::Complete, ScanStatus::Incomplete),
        work_used: 0,
        findings: Vec::new(),
        driver_candidates: Vec::new(),
        limitations: &[
            "Four fixed CGB MBC5 layouts provide table/header evidence. A separate public fingerprint pack identifies possible additional sound-driver families.",
            "Fingerprint candidates report matching text, instruction bytes, instrument data or header identifiers. They do not establish an active driver, song table, playback contract or complete soundtrack.",
            "Only the first occurrence of each fingerprint is retained. Multiple required fingerprints may occur in different banks or embedded programs.",
            "Driver fingerprints and mapped table/header pointers do not validate sequence commands, runtime behavior, song boundaries or complete soundtrack coverage.",
            "The inspected table extent comes from a known layout; it is not an inferred table terminator.",
            "Known-ROM qualification lists only existing native playback selectors. It does not qualify GBS export or other selectors.",
            "Evidence cannot be used as a playback selection. Native preparation independently authenticates the complete source and measured selector metadata.",
            "This evidence pass has its own work and candidate limits. The bounded media-identity hash is a separate pass.",
        ],
    };
    if stop.is_some() {
        return report;
    }
    if system != System::Gb {
        report.status = ScanStatus::Unsupported;
        return report;
    }
    let mut budget = Budget {
        cancel,
        remaining: limits.max_work,
    };
    let result = crate::gb_native::detect_drivers(
        bytes,
        report.media.sha256.as_deref(),
        &mut report.findings,
        &mut budget,
        limits.max_candidates as usize,
    )
    .and_then(|()| {
        gb_fingerprints::scan(
            bytes,
            &mut report.driver_candidates,
            &mut budget,
            limits.max_candidates as usize - report.findings.len(),
        )
    });
    if let Err(stop) = result {
        report.status = ScanStatus::Incomplete(stop);
    }
    report.work_used = limits.max_work - budget.remaining;
    report
}

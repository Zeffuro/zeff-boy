use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{DetectorState, MalformedInput, ScanReport, ScanStatus, ScanStop};

mod code;
pub use code::CodeCoverage;

/// `RenderSupported` means WAV is offered with no pending runtime validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStage {
    NoCandidate,
    DriverEvidence,
    Catalogued,
    RenderSupported,
}

impl CoverageStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoCandidate => "no_candidate",
            Self::DriverEvidence => "driver_evidence",
            Self::Catalogued => "catalogued",
            Self::RenderSupported => "render_supported",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum CoverageBlocker {
    ScanIncomplete(CoverageStop),
    ScanMalformed(CoverageMalformed),
    ScanUnsupported,
    DetectorIncomplete {
        id: String,
        reason: CoverageStop,
    },
    DetectorMalformed {
        id: String,
        reason: CoverageMalformed,
    },
    DetectorUnsupported {
        id: String,
    },
    DetectorNotRun {
        id: String,
        reason: CoverageStop,
    },
    DetectorNotReported {
        id: String,
    },
    NoCatalogEntries,
    DriverEvidenceOnly,
    CatalogRuntimeValidationRequired,
    CatalogRenderUnsupported,
}

impl CoverageBlocker {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ScanIncomplete(_) => "scan_incomplete",
            Self::ScanMalformed(_) => "scan_malformed",
            Self::ScanUnsupported => "scan_unsupported",
            Self::DetectorIncomplete { .. } => "detector_incomplete",
            Self::DetectorMalformed { .. } => "detector_malformed",
            Self::DetectorUnsupported { .. } => "detector_unsupported",
            Self::DetectorNotRun { .. } => "detector_not_run",
            Self::DetectorNotReported { .. } => "detector_not_reported",
            Self::NoCatalogEntries => "no_catalog_entries",
            Self::DriverEvidenceOnly => "driver_evidence_only",
            Self::CatalogRuntimeValidationRequired => "catalog_runtime_validation_required",
            Self::CatalogRenderUnsupported => "catalog_render_unsupported",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStop {
    Cancelled,
    WorkLimit,
    CandidateLimit,
    MediaLimit,
    InvalidLimits,
    InventoryLimit,
    ValidationLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageMalformed {
    TruncatedChunk,
    InvalidChunkSize,
    InvalidChunkOrder,
    MissingRequiredChunk,
    DuplicateChunk,
    TruncatedHeader,
    EmptyProgram,
    InvalidSongCount,
    InvalidFirstSong,
    InvalidAddress,
    ProgramLengthExceedsSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum DetectorCoverageState {
    Complete,
    Unsupported,
    Malformed(CoverageMalformed),
    Incomplete(CoverageStop),
    NotRun(CoverageStop),
    NotReported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectorCoverage {
    pub id: String,
    pub semantic_version: u32,
    pub state: DetectorCoverageState,
    pub retained_matches: u32,
    pub catalog_entries: usize,
    pub render_supported_entries: usize,
    #[serde(default)]
    pub pending_runtime_entries: usize,
    pub work_used: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineCoverage {
    pub engine: String,
    pub catalog_entries: usize,
    pub render_supported_entries: usize,
    #[serde(default)]
    pub pending_runtime_entries: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub system: String,
    pub media_sha256: Option<String>,
    pub catalog_entries: usize,
    pub render_supported_entries: usize,
    #[serde(default)]
    pub pending_runtime_entries: usize,
    pub driver_candidates: usize,
    pub driver_families: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structural: Option<StructuralCoverage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<CodeCoverage>,
    pub stage: CoverageStage,
    pub blockers: Vec<CoverageBlocker>,
    pub detectors: Vec<DetectorCoverage>,
    pub engines: Vec<EngineCoverage>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralCoverage {
    pub inventories: usize,
    pub selectors: usize,
    pub held_selectors: usize,
}

pub fn observe(report: &ScanReport) -> Observation {
    let mut detectors = BTreeMap::new();
    for descriptor in report.applicable_detectors {
        detectors.insert(
            descriptor.id.to_owned(),
            DetectorCoverage {
                id: descriptor.id.to_owned(),
                semantic_version: descriptor.semantic_version,
                state: DetectorCoverageState::NotReported,
                retained_matches: 0,
                catalog_entries: 0,
                render_supported_entries: 0,
                pending_runtime_entries: 0,
                work_used: 0,
            },
        );
    }
    for outcome in &report.detector_outcomes {
        detectors.insert(
            outcome.descriptor.id.to_owned(),
            DetectorCoverage {
                id: outcome.descriptor.id.to_owned(),
                semantic_version: outcome.descriptor.semantic_version,
                state: outcome.state.into(),
                retained_matches: outcome.retained_matches,
                catalog_entries: 0,
                render_supported_entries: 0,
                pending_runtime_entries: 0,
                work_used: outcome.work_used,
            },
        );
    }

    let mut engines: BTreeMap<String, EngineCoverage> = BTreeMap::new();
    let mut catalog_entries = 0;
    let mut pending_runtime_entries = 0;
    for finding in report.catalog() {
        catalog_entries += 1;
        let pending = finding.runtime_validation_required;
        let supports_wav = finding.supports_format("wav") && !pending;
        let detector = detectors
            .entry(finding.detector_id.clone())
            .or_insert_with(|| DetectorCoverage {
                id: finding.detector_id.clone(),
                semantic_version: 0,
                state: DetectorCoverageState::NotReported,
                retained_matches: 0,
                catalog_entries: 0,
                render_supported_entries: 0,
                pending_runtime_entries: 0,
                work_used: 0,
            });
        detector.catalog_entries += 1;
        detector.render_supported_entries += usize::from(supports_wav);
        detector.pending_runtime_entries += usize::from(pending);

        let engine = engines
            .entry(finding.engine_hint.clone())
            .or_insert(EngineCoverage {
                engine: finding.engine_hint.clone(),
                catalog_entries: 0,
                render_supported_entries: 0,
                pending_runtime_entries: 0,
            });
        engine.catalog_entries += 1;
        engine.render_supported_entries += usize::from(supports_wav);
        engine.pending_runtime_entries += usize::from(pending);
        pending_runtime_entries += usize::from(pending);
    }

    let render_supported_entries = detectors
        .values()
        .map(|coverage| coverage.render_supported_entries)
        .sum();
    let mut driver_families = BTreeMap::new();
    let mut structural = StructuralCoverage::default();
    let mut code = CodeCoverage::default();
    for candidate in &report.driver_candidates {
        *driver_families
            .entry(candidate.family.to_owned())
            .or_insert(0) += 1;
        code.add(candidate);
        if let Some(inventory) = &candidate.inventory {
            structural.inventories += 1;
            structural.selectors += inventory.entries.len();
            structural.held_selectors += inventory.held.len();
        }
    }
    let mut blockers = scan_blockers(report);
    for detector in detectors.values() {
        match detector.state {
            DetectorCoverageState::Complete => {}
            DetectorCoverageState::NotReported => {
                blockers.push(CoverageBlocker::DetectorNotReported {
                    id: detector.id.clone(),
                });
            }
            DetectorCoverageState::Unsupported => {
                blockers.push(CoverageBlocker::DetectorUnsupported {
                    id: detector.id.clone(),
                })
            }
            DetectorCoverageState::Malformed(reason) => {
                blockers.push(CoverageBlocker::DetectorMalformed {
                    id: detector.id.clone(),
                    reason,
                });
            }
            DetectorCoverageState::Incomplete(reason) => {
                blockers.push(CoverageBlocker::DetectorIncomplete {
                    id: detector.id.clone(),
                    reason,
                });
            }
            DetectorCoverageState::NotRun(reason) => {
                blockers.push(CoverageBlocker::DetectorNotRun {
                    id: detector.id.clone(),
                    reason,
                })
            }
        }
    }
    if catalog_entries == 0 {
        blockers.push(CoverageBlocker::NoCatalogEntries);
        if !report.driver_candidates.is_empty() {
            blockers.push(CoverageBlocker::DriverEvidenceOnly);
        }
    } else if render_supported_entries == 0 {
        blockers.push(if pending_runtime_entries != 0 {
            CoverageBlocker::CatalogRuntimeValidationRequired
        } else {
            CoverageBlocker::CatalogRenderUnsupported
        });
    }

    Observation {
        system: report.media.system.to_owned(),
        media_sha256: report.media.sha256.clone(),
        catalog_entries,
        render_supported_entries,
        pending_runtime_entries,
        driver_candidates: report.driver_candidates.len(),
        driver_families,
        structural: (structural.inventories != 0).then_some(structural),
        code: (code.candidates != 0).then_some(code),
        stage: stage(
            catalog_entries,
            render_supported_entries,
            report.driver_candidates.len(),
        ),
        blockers,
        detectors: detectors.into_values().collect(),
        engines: engines.into_values().collect(),
    }
}

fn stage(
    catalog_entries: usize,
    render_supported_entries: usize,
    driver_candidates: usize,
) -> CoverageStage {
    if render_supported_entries != 0 {
        CoverageStage::RenderSupported
    } else if catalog_entries != 0 {
        CoverageStage::Catalogued
    } else if driver_candidates != 0 {
        CoverageStage::DriverEvidence
    } else {
        CoverageStage::NoCandidate
    }
}

fn scan_blockers(report: &ScanReport) -> Vec<CoverageBlocker> {
    match report.status {
        ScanStatus::Complete => Vec::new(),
        ScanStatus::Unsupported => vec![CoverageBlocker::ScanUnsupported],
        ScanStatus::Malformed(reason) => vec![CoverageBlocker::ScanMalformed(reason.into())],
        ScanStatus::Incomplete(reason) => vec![CoverageBlocker::ScanIncomplete(reason.into())],
    }
}

impl From<DetectorState> for DetectorCoverageState {
    fn from(state: DetectorState) -> Self {
        match state {
            DetectorState::Complete => Self::Complete,
            DetectorState::Unsupported => Self::Unsupported,
            DetectorState::Malformed(reason) => Self::Malformed(reason.into()),
            DetectorState::Incomplete(reason) => Self::Incomplete(reason.into()),
            DetectorState::NotRun(reason) => Self::NotRun(reason.into()),
        }
    }
}

impl From<ScanStop> for CoverageStop {
    fn from(stop: ScanStop) -> Self {
        match stop {
            ScanStop::Cancelled => Self::Cancelled,
            ScanStop::WorkLimit => Self::WorkLimit,
            ScanStop::CandidateLimit => Self::CandidateLimit,
            ScanStop::MediaLimit => Self::MediaLimit,
            ScanStop::InvalidLimits => Self::InvalidLimits,
            ScanStop::InventoryLimit => Self::InventoryLimit,
            ScanStop::ValidationLimit => Self::ValidationLimit,
        }
    }
}

impl From<MalformedInput> for CoverageMalformed {
    fn from(malformed: MalformedInput) -> Self {
        match malformed {
            MalformedInput::TruncatedChunk => Self::TruncatedChunk,
            MalformedInput::InvalidChunkSize => Self::InvalidChunkSize,
            MalformedInput::InvalidChunkOrder => Self::InvalidChunkOrder,
            MalformedInput::MissingRequiredChunk => Self::MissingRequiredChunk,
            MalformedInput::DuplicateChunk => Self::DuplicateChunk,
            MalformedInput::TruncatedHeader => Self::TruncatedHeader,
            MalformedInput::EmptyProgram => Self::EmptyProgram,
            MalformedInput::InvalidSongCount => Self::InvalidSongCount,
            MalformedInput::InvalidFirstSong => Self::InvalidFirstSong,
            MalformedInput::InvalidAddress => Self::InvalidAddress,
            MalformedInput::ProgramLengthExceedsSource => Self::ProgramLengthExceedsSource,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::{ScanLimits, scan};
    use zeff_emu_common::system::System;

    #[test]
    fn incomplete_scan_keeps_diagnostics_when_nothing_is_catalogued() {
        let report = scan(
            System::Gba,
            &crate::test_support::fixture(),
            ScanLimits {
                max_work: 0,
                ..ScanLimits::default()
            },
            &AtomicBool::new(false),
        );

        let observation = observe(&report);
        assert_eq!(observation.stage, CoverageStage::NoCandidate);
        assert_eq!(observation.catalog_entries, 0);
        assert!(
            observation
                .blockers
                .contains(&CoverageBlocker::ScanIncomplete(CoverageStop::WorkLimit))
        );
        assert!(
            observation
                .blockers
                .contains(&CoverageBlocker::NoCatalogEntries)
        );
        assert!(observation.detectors.iter().all(|detector| matches!(
            detector.state,
            DetectorCoverageState::Incomplete(CoverageStop::WorkLimit)
                | DetectorCoverageState::NotRun(CoverageStop::WorkLimit)
        )));
    }

    #[test]
    fn fingerprint_evidence_does_not_become_a_catalogue_entry() {
        let mut report = scan(
            System::Gb,
            &[],
            ScanLimits::default(),
            &AtomicBool::new(false),
        );
        report
            .driver_candidates
            .push(crate::drivers::gb_fingerprints::DriverCandidate {
                family: "test",
                variant: "test",
                qualification:
                    crate::drivers::gb_fingerprints::FingerprintQualification::FingerprintOnly,
                fingerprint_source: "test",
                fingerprint_revision: "test",
                evidence: Vec::new(),
                inventory: None,
                code: None,
            });

        let observation = observe(&report);
        assert_eq!(observation.stage, CoverageStage::DriverEvidence);
        assert_eq!(observation.catalog_entries, 0);
        assert_eq!(observation.render_supported_entries, 0);
        assert_eq!(observation.driver_families.get("test"), Some(&1));
        assert!(
            observation
                .blockers
                .contains(&CoverageBlocker::NoCatalogEntries)
        );
        assert!(
            observation
                .blockers
                .contains(&CoverageBlocker::DriverEvidenceOnly)
        );
    }

    #[test]
    fn missing_detector_outcomes_remain_visible() {
        let mut report = scan(
            System::Gb,
            &[],
            ScanLimits::default(),
            &AtomicBool::new(false),
        );
        report.detector_outcomes.clear();
        let observation = observe(&report);
        for descriptor in report.applicable_detectors {
            assert!(
                observation
                    .blockers
                    .contains(&CoverageBlocker::DetectorNotReported {
                        id: descriptor.id.to_owned(),
                    })
            );
        }
    }

    #[test]
    fn catalogued_entries_without_wav_capability_are_explicit() {
        let report = scan(
            System::Gba,
            &crate::test_support::tracker::xm_fixture(),
            ScanLimits::default(),
            &AtomicBool::new(false),
        );

        let observation = observe(&report);
        assert_eq!(observation.stage, CoverageStage::Catalogued);
        assert_eq!(observation.catalog_entries, 1);
        assert_eq!(observation.render_supported_entries, 0);
        assert!(
            observation
                .blockers
                .contains(&CoverageBlocker::CatalogRenderUnsupported)
        );
    }

    #[test]
    fn scan_status_remains_visible_with_catalogued_entries() {
        for (status, blocker) in [
            (ScanStatus::Unsupported, CoverageBlocker::ScanUnsupported),
            (
                ScanStatus::Malformed(MalformedInput::TruncatedHeader),
                CoverageBlocker::ScanMalformed(CoverageMalformed::TruncatedHeader),
            ),
            (
                ScanStatus::Incomplete(ScanStop::ValidationLimit),
                CoverageBlocker::ScanIncomplete(CoverageStop::ValidationLimit),
            ),
        ] {
            let mut report = scan(
                System::Gba,
                &crate::test_support::fixture(),
                ScanLimits::default(),
                &AtomicBool::new(false),
            );
            report.status = status;

            let observation = observe(&report);
            assert_eq!(observation.catalog_entries, 1);
            assert_eq!(observation.render_supported_entries, 1);
            assert!(observation.blockers.contains(&blocker));
        }
    }

    #[test]
    fn catalogued_entries_count_wav_capability_by_detector_and_engine() {
        let mut report = scan(
            System::Gba,
            &crate::test_support::fixture(),
            ScanLimits::default(),
            &AtomicBool::new(false),
        );
        let tracker = scan(
            System::Gba,
            &crate::test_support::tracker::xm_fixture(),
            ScanLimits::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(report.song_count(), 1);
        assert_eq!(tracker.tracker_modules.len(), 1);
        report.tracker_modules = tracker.tracker_modules;

        let observation = observe(&report);
        assert_eq!(observation.stage, CoverageStage::RenderSupported);
        assert_eq!(observation.catalog_entries, 2);
        assert_eq!(observation.render_supported_entries, 1);
        assert_eq!(
            observation
                .detectors
                .iter()
                .find(|detector| detector.id == "mp2k-sequence")
                .unwrap()
                .render_supported_entries,
            1
        );
        assert_eq!(
            observation
                .detectors
                .iter()
                .find(|detector| detector.id == "tracker-structure")
                .unwrap()
                .render_supported_entries,
            0
        );
        let encoded = serde_json::to_string(&observation).unwrap();
        assert_eq!(
            serde_json::from_str::<Observation>(&encoded).unwrap(),
            observation
        );
    }
}

use serde::Serialize;

use crate::{
    ScanReport, SourceSpan,
    catalog::{SongId, SongRef},
    drivers::{
        CandidateQualification as DriverCandidateQualification, CodeInventory, DriverCandidate,
        EvidenceKind, StructuralInventory,
    },
    formats::SONG_FORMATS,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingPayloadKind {
    Sequence,
    NativeExecutable,
    RecordedLog,
    Sample,
    Module,
    Container,
    CdAudio,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FindingCapability {
    pub format: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub id: SongId,
    pub detector_id: String,
    pub payload_kind: FindingPayloadKind,
    pub source_span: Option<SourceSpan>,
    pub title_hint: String,
    pub engine_hint: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub runtime_validation_required: bool,
    pub capabilities: Vec<FindingCapability>,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl Finding {
    pub fn supports_format(&self, format: &str) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability.format == format)
    }

    fn from_song(id: SongId, song: SongRef<'_>) -> Self {
        Self {
            id,
            detector_id: song.detector_id().to_owned(),
            payload_kind: payload_kind(song),
            source_span: song.span(),
            title_hint: song.title(),
            engine_hint: song.engine().to_owned(),
            runtime_validation_required: song.requires_runtime_validation(),
            capabilities: SONG_FORMATS
                .iter()
                .filter(|info| song.supports(info.format))
                .map(|info| FindingCapability {
                    format: info.id.to_owned(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CandidateFinding {
    pub family_hint: String,
    pub variant_hint: String,
    pub qualification: CandidateQualification,
    pub fingerprint_source: String,
    pub fingerprint_revision: String,
    pub evidence: Vec<CandidateEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory: Option<StructuralInventory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<CodeInventory>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateQualification {
    FingerprintOnly,
    StaticCode,
    Structural,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CandidateEvidence {
    pub signature: String,
    pub kind: CandidateEvidenceKind,
    pub source_span: SourceSpan,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateEvidenceKind {
    TextIdentifier,
    InstructionBytes,
    InstrumentData,
    HeaderIdentifier,
    SoundRegisterWrite,
    DirectCall,
    DriverData,
}

impl From<&DriverCandidate> for CandidateFinding {
    fn from(candidate: &DriverCandidate) -> Self {
        Self {
            family_hint: candidate.family.to_owned(),
            variant_hint: candidate.variant.to_owned(),
            qualification: candidate.qualification.into(),
            fingerprint_source: candidate.fingerprint_source.to_owned(),
            fingerprint_revision: candidate.fingerprint_revision.to_owned(),
            evidence: candidate
                .evidence
                .iter()
                .map(|evidence| CandidateEvidence {
                    signature: evidence.signature.to_owned(),
                    kind: evidence.kind.into(),
                    source_span: evidence.span.into(),
                    sha256: evidence.sha256.clone(),
                })
                .collect(),
            inventory: candidate.inventory.clone(),
            code: candidate.code.clone(),
        }
    }
}

impl From<DriverCandidateQualification> for CandidateQualification {
    fn from(qualification: DriverCandidateQualification) -> Self {
        match qualification {
            DriverCandidateQualification::FingerprintOnly => Self::FingerprintOnly,
            DriverCandidateQualification::StaticCode => Self::StaticCode,
            DriverCandidateQualification::Structural => Self::Structural,
        }
    }
}

impl From<EvidenceKind> for CandidateEvidenceKind {
    fn from(kind: EvidenceKind) -> Self {
        match kind {
            EvidenceKind::TextIdentifier => Self::TextIdentifier,
            EvidenceKind::InstructionBytes => Self::InstructionBytes,
            EvidenceKind::InstrumentData => Self::InstrumentData,
            EvidenceKind::HeaderIdentifier => Self::HeaderIdentifier,
            EvidenceKind::SoundRegisterWrite => Self::SoundRegisterWrite,
            EvidenceKind::DirectCall => Self::DirectCall,
            EvidenceKind::DriverData => Self::DriverData,
        }
    }
}

impl ScanReport {
    pub fn catalog(&self) -> impl Iterator<Item = Finding> + '_ {
        self.song_ids()
            .map(|id| Finding::from_song(id, self.song(id).expect("song IDs always resolve")))
    }

    pub fn candidate_findings(&self) -> impl Iterator<Item = CandidateFinding> + '_ {
        self.driver_candidates.iter().map(CandidateFinding::from)
    }
}

fn payload_kind(song: SongRef<'_>) -> FindingPayloadKind {
    match song {
        SongRef::Vgm(_) => FindingPayloadKind::RecordedLog,
        SongRef::AasStream(_) | SongRef::AasPcm(_) => FindingPayloadKind::Sample,
        SongRef::Module(_) => FindingPayloadKind::Module,
        SongRef::Rip(_) => FindingPayloadKind::Container,
        #[cfg(not(target_arch = "wasm32"))]
        SongRef::Cdda(_) => FindingPayloadKind::CdAudio,
        SongRef::Krawall(_)
        | SongRef::GaxNative(_)
        | SongRef::Musyx(_)
        | SongRef::Aas(_)
        | SongRef::DescriptorMidi(_)
        | SongRef::Nsq(_)
        | SongRef::Radriver(_)
        | SongRef::Gbass(_)
        | SongRef::NesNative(_)
        | SongRef::GbNative(_)
        | SongRef::GbMusyx(_)
        | SongRef::GbTose(_)
        | SongRef::GbQuickThunder(_)
        | SongRef::GbGhx(_)
        | SongRef::GbSoundSystem(_)
        | SongRef::GbCarillon(_)
        | SongRef::WsTose(_)
        | SongRef::NesTose(_)
        | SongRef::SegaPsg(_) => FindingPayloadKind::NativeExecutable,
        SongRef::Huge(_) => FindingPayloadKind::Sequence,
        SongRef::Gb(song) => {
            if crate::gb_music::native::supports_native(song) {
                FindingPayloadKind::NativeExecutable
            } else {
                FindingPayloadKind::Sequence
            }
        }
        SongRef::Nes(song) => {
            if crate::nes_music::native::supports_native(song) {
                FindingPayloadKind::NativeExecutable
            } else {
                FindingPayloadKind::Sequence
            }
        }
        SongRef::Mp2k(_) | SongRef::Gax(_) | SongRef::EngineSoftware(_) | SongRef::Natsume(_) => {
            FindingPayloadKind::Sequence
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::{ScanLimits, formats::SONG_FORMATS, scan};
    use zeff_emu_common::system::System;

    #[test]
    fn findings_preserve_every_catalogue_selection_and_supported_operation() {
        let report = scan(
            System::Gba,
            &crate::test_support::fixture(),
            ScanLimits::default(),
            &AtomicBool::new(false),
        );

        let findings: Vec<_> = report.catalog().collect();
        assert_eq!(findings.len(), report.song_count());
        for finding in findings {
            let song = report.song(finding.id).expect("finding IDs are stable");
            assert_eq!(finding.detector_id, song.detector_id());
            assert_eq!(finding.source_span, song.span());
            assert_eq!(finding.title_hint, song.title());
            assert_eq!(finding.engine_hint, song.engine());
            assert_eq!(
                finding.capabilities,
                SONG_FORMATS
                    .iter()
                    .filter(|info| song.supports(info.format))
                    .map(|info| FindingCapability {
                        format: info.id.to_owned(),
                    })
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn module_capabilities_do_not_claim_unsupported_rendering() {
        let report = scan(
            System::Gba,
            &crate::test_support::tracker::xm_fixture(),
            ScanLimits::default(),
            &AtomicBool::new(false),
        );

        let finding = report.catalog().next().unwrap();
        assert_eq!(finding.payload_kind, FindingPayloadKind::Module);
        assert!(finding.supports_format("xm"));
        assert!(finding.supports_format("assets"));
        assert!(!finding.supports_format("wav"));
        let encoded = serde_json::to_value(&finding).unwrap();
        assert!(encoded.get("title_hint").is_some());
        assert!(encoded.get("engine_hint").is_some());
        assert!(encoded.get("runtime_validated").is_none());
        assert!(encoded.get("playback_validated").is_none());
    }

    #[test]
    fn qualified_sega_driver_is_executable_while_mp2k_retains_sequence_payload() {
        let cancel = AtomicBool::new(false);
        let report = scan(
            System::Sms,
            &crate::sega_psg::fixture_rom(),
            ScanLimits::default(),
            &cancel,
        );
        let finding = report
            .catalog()
            .find(|finding| matches!(finding.id, SongId::SegaPsg(_)))
            .unwrap();
        assert_eq!(finding.payload_kind, FindingPayloadKind::NativeExecutable);
        assert!(finding.supports_format("sgc"));
        let report = scan(
            System::Gba,
            &crate::test_support::fixture(),
            ScanLimits::default(),
            &cancel,
        );
        assert_eq!(
            report
                .catalog()
                .find(|finding| matches!(finding.id, SongId::Mp2k(_)))
                .unwrap()
                .payload_kind,
            FindingPayloadKind::Sequence
        );
    }

    #[test]
    fn fingerprint_candidates_remain_outside_catalogue_ids() {
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
                qualification: DriverCandidateQualification::FingerprintOnly,
                fingerprint_source: "test",
                fingerprint_revision: "test",
                evidence: Vec::new(),
                inventory: None,
                code: None,
            });

        assert_eq!(report.catalog().count(), 0);
        assert_eq!(report.candidate_findings().count(), 1);
        assert!(matches!(
            report.candidate_findings().next().unwrap().qualification,
            CandidateQualification::FingerprintOnly
        ));
    }
}

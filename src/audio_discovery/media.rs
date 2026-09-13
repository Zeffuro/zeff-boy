use std::sync::{Arc, atomic::AtomicBool};

use serde::Serialize;
use zeff_emu_common::system::System;

use super::tracker::EmbeddedFormat;
use super::{
    DetectorState, MediaIdentity, ScanLimits, ScanReport, ScanStatus, ScanStop, scan,
    scan_standalone_tracker,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StandaloneFormat {
    Tracker(EmbeddedFormat),
    Vgm,
    Rip(super::rips::RipFormat),
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod import;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SourceIdentity {
    pub(crate) kind: &'static str,
    pub(crate) sha256: String,
    pub(crate) len: usize,
    pub(crate) container: Option<ContainerIdentity>,
    pub(crate) selected_member: Option<SelectedMemberIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ContainerIdentity {
    pub(crate) format: &'static str,
    pub(crate) sha256: String,
    pub(crate) len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SelectedMemberIdentity {
    pub(crate) name: String,
    pub(crate) sha256: String,
    pub(crate) len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScanProvenance {
    pub(crate) source: SourceIdentity,
    pub(crate) transforms: Vec<crate::mods::ModApplicationStep>,
}

pub(crate) struct ScanInput {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cdda: Option<Arc<super::cdda::CdAudioInput>>,
    pub(crate) system: Option<System>,
    pub(crate) standalone_audio: Option<StandaloneFormat>,
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) provenance: Option<Arc<ScanProvenance>>,
    pub(crate) analysis_profile: &'static str,
    pub(crate) display_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ScanManifest {
    pub(crate) schema: &'static str,
    pub(crate) analysis_profile: &'static str,
    pub(crate) source: Option<SourceIdentity>,
    pub(crate) transforms: Option<Vec<crate::mods::ModApplicationStep>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) display_name: Option<String>,
    pub(crate) scan: ScanReport,
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) disc: Option<DiscIdentity>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiscIdentity {
    pub(crate) original_disc_sha256: String,
    pub(crate) original_disc_len: Option<usize>,
    pub(crate) effective_disc_sha256: String,
    pub(crate) effective_disc_len: usize,
    pub(crate) provenance: super::cdda::CdAudioProvenance,
}

impl ScanInput {
    pub(crate) fn media_system_id(&self) -> &'static str {
        self.system.map_or_else(
            || match self.standalone_audio {
                Some(StandaloneFormat::Vgm) => "standalone_vgm",
                Some(StandaloneFormat::Rip(format)) => format.system_id(),
                _ => "standalone_tracker",
            },
            System::code,
        )
    }

    pub(crate) fn analyze(&self, limits: ScanLimits, cancel: &AtomicBool) -> ScanManifest {
        ScanManifest {
            schema: "zeff-audio-discovery/1",
            analysis_profile: self.analysis_profile,
            source: self.provenance.as_ref().map(|value| value.source.clone()),
            transforms: self
                .provenance
                .as_ref()
                .map(|value| value.transforms.clone()),
            display_name: self.display_name.clone(),
            scan: self.scan(limits, cancel),
            #[cfg(not(target_arch = "wasm32"))]
            disc: self.cdda.as_ref().map(|input| DiscIdentity {
                original_disc_sha256: input.original_disc_sha256.clone(),
                original_disc_len: input.original_disc_len,
                effective_disc_sha256: input.effective_disc_sha256.clone(),
                effective_disc_len: input.effective_disc_len,
                provenance: input.provenance.clone(),
            }),
        }
    }

    fn scan(&self, limits: ScanLimits, cancel: &AtomicBool) -> ScanReport {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(input) = &self.cdda {
            return scan_disc(
                input,
                limits,
                cancel,
                self.system == Some(System::Pce)
                    && self.standalone_audio.is_none()
                    && self.bytes.is_empty(),
            );
        }
        match (self.system, self.standalone_audio) {
            (Some(system), None) => scan(system, &self.bytes, limits, cancel),
            (None, Some(StandaloneFormat::Tracker(format))) => {
                scan_standalone_tracker(&self.bytes, format, limits, cancel)
            }
            (None, Some(StandaloneFormat::Vgm)) => super::vgm::scan(&self.bytes, limits, cancel),
            (None, Some(StandaloneFormat::Rip(format))) => {
                super::rips::scan(&self.bytes, format, limits, cancel)
            }
            _ => invalid_input_report(limits, cancel),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn from_disc(
        input: super::cdda::CdAudioInput,
        analysis_profile: &'static str,
    ) -> Self {
        Self {
            cdda: Some(Arc::new(input)),
            system: Some(System::Pce),
            standalone_audio: None,
            bytes: Arc::from([]),
            provenance: None,
            analysis_profile,
            display_name: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn standalone_tracker(
        bytes: Vec<u8>,
        source: SourceIdentity,
        format: EmbeddedFormat,
        display_name: Option<String>,
    ) -> Self {
        Self::standalone(
            bytes,
            source,
            StandaloneFormat::Tracker(format),
            display_name,
        )
    }

    pub(crate) fn standalone(
        bytes: Vec<u8>,
        source: SourceIdentity,
        format: StandaloneFormat,
        display_name: Option<String>,
    ) -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            cdda: None,
            system: None,
            standalone_audio: Some(format),
            bytes: bytes.into(),
            provenance: Some(Arc::new(ScanProvenance {
                source,
                transforms: Vec::new(),
            })),
            analysis_profile: "standalone-unmodified-v1",
            display_name,
        }
    }
}

fn invalid_input_report(limits: ScanLimits, cancel: &AtomicBool) -> ScanReport {
    let mut report = ScanReport::new(
        "invalid-media-input",
        1,
        &[],
        &["Conflicting or missing media selections are rejected before a detector runs."],
        MediaIdentity {
            system: "invalid_media_input",
            byte_len: 0,
            sha256: None,
        },
        limits,
    );
    if report.preflight(cancel).is_none() {
        report.status = ScanStatus::Incomplete(ScanStop::InvalidLimits);
        report.finish_not_run(ScanStop::InvalidLimits);
    }
    report
}

#[cfg(not(target_arch = "wasm32"))]
fn scan_disc(
    input: &super::cdda::CdAudioInput,
    limits: ScanLimits,
    cancel: &AtomicBool,
    valid_source: bool,
) -> ScanReport {
    let mut report = ScanReport::new(
        "pce-cdda-toc",
        2,
        super::detectors::CDDA,
        &[
            "Audio tracks come from the loaded table of contents. Cartridge PSG sequences and CD ADPCM assets are not identified.",
            "Exports begin at index 1 and omit pregaps. The exact stored audio-track payload, including retained pregap sectors, is verified against its loaded hash before encoding.",
            "WAV and FLAC preserve 44.1 kHz stereo PCM; Ogg Vorbis is lossy. No emulation, mixing, fades or gain changes are applied.",
        ],
        MediaIdentity {
            system: "pce",
            byte_len: input.effective_disc_len as u64,
            sha256: (!cancel.load(std::sync::atomic::Ordering::Relaxed))
                .then(|| input.effective_disc_sha256.clone()),
        },
        limits,
    );
    if report
        .preflight_with_media_limit(cancel, u64::MAX)
        .is_some()
    {
        return report;
    }
    if !valid_source {
        report.status = ScanStatus::Incomplete(ScanStop::InvalidLimits);
        report.finish_not_run(ScanStop::InvalidLimits);
        return report;
    }
    let mut budget = super::Budget {
        cancel,
        remaining: limits.max_work,
    };
    match input.tracks() {
        Ok(tracks) => {
            for track in tracks {
                if let Err(reason) = budget.charge() {
                    report.status = ScanStatus::Incomplete(reason);
                    break;
                }
                if report.cdda_tracks.len() >= limits.max_candidates as usize {
                    report.status = ScanStatus::Incomplete(ScanStop::CandidateLimit);
                    break;
                }
                report.cdda_tracks.push(track);
            }
        }
        Err(_) => report.status = ScanStatus::Incomplete(ScanStop::ValidationLimit),
    }
    report.work_used = limits.max_work - budget.remaining;
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        report.status = ScanStatus::Incomplete(ScanStop::Cancelled);
    }
    let state = match report.status {
        ScanStatus::Complete => DetectorState::Complete,
        ScanStatus::Incomplete(reason) => DetectorState::Incomplete(reason),
        ScanStatus::Unsupported | ScanStatus::Malformed(_) => {
            unreachable!("CDDA scanning is either complete or incomplete")
        }
    };
    report.record_detector(
        super::detectors::CDDA[0],
        state,
        report.cdda_tracks.len(),
        report.work_used,
    );
    debug_assert_eq!(
        report.detector_outcomes.len(),
        report.applicable_detectors.len()
    );
    report
}

#[cfg(not(target_arch = "wasm32"))]
impl ScanManifest {
    pub(crate) fn write_new(&self, path: &std::path::Path) -> anyhow::Result<()> {
        use anyhow::Context;
        use std::io::Seek;

        let bytes = serde_json::to_vec_pretty(&serde_json::to_value(self)?)?;
        crate::platform::write_new_file_atomically_validated(path, &bytes, |file| {
            file.rewind()?;
            let _: serde_json::Value = serde_json::from_reader(file)?;
            Ok(())
        })
        .with_context(|| format!("failed to create audio discovery report {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflicting_media_targets_fail_closed_without_a_console_scan() {
        let input = ScanInput {
            #[cfg(not(target_arch = "wasm32"))]
            cdda: None,
            system: Some(System::Gba),
            standalone_audio: Some(StandaloneFormat::Tracker(EmbeddedFormat::Xm)),
            bytes: Arc::from([]),
            provenance: None,
            analysis_profile: "test",
            display_name: None,
        };
        let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        assert_eq!(
            manifest.scan.status,
            super::super::ScanStatus::Incomplete(super::super::ScanStop::InvalidLimits)
        );
        assert!(manifest.scan.tracker_modules.is_empty());
        assert!(manifest.scan.candidates.is_empty());
        assert!(manifest.scan.applicable_detectors.is_empty());
        assert!(manifest.scan.detector_outcomes.is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cdda_identity_has_only_its_own_detector_outcome() {
        use std::sync::Arc;

        use zeff_pce_core::hardware::{CdDisc, CdTrack, CdTrackMode};

        let disc = Arc::new(
            CdDisc::new(vec![
                CdTrack::from_index1_data(1, 0, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048])
                    .unwrap(),
            ])
            .unwrap(),
        );
        let audio = super::super::cdda::CdAudioInput::new(
            Arc::clone(&disc),
            "01".repeat(32),
            Some(2048),
            const_hex::encode(disc.content_hash()),
            super::super::cdda::CdAudioProvenance {
                source_kind: "test",
                source_media_sha256: "02".repeat(32),
                source_media_len: 2048,
                selected_member_path_sha256: None,
                transforms_applied: false,
            },
        )
        .unwrap();

        let mut input = ScanInput::from_disc(audio, "test");
        let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        assert_eq!(manifest.scan.status, super::super::ScanStatus::Complete);
        assert_eq!(manifest.scan.media.system, input.media_system_id());
        assert!(manifest.scan.tracker_modules.is_empty());
        assert_eq!(manifest.scan.detector_outcomes.len(), 1);
        assert_eq!(
            manifest.scan.detector_outcomes[0].descriptor.id,
            "pce-cdda-toc"
        );
        assert_eq!(
            manifest.scan.detector_outcomes[0].state,
            super::super::DetectorState::Complete
        );
        Arc::make_mut(input.cdda.as_mut().unwrap()).effective_disc_len =
            super::super::MAX_ROM_BYTES + 1;
        let large = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        assert_eq!(large.scan.status, ScanStatus::Complete);
        input.standalone_audio = Some(StandaloneFormat::Vgm);
        let conflicting = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        assert_eq!(
            conflicting.scan.status,
            ScanStatus::Incomplete(ScanStop::InvalidLimits)
        );
        assert_eq!(
            conflicting.scan.detector_outcomes[0].state,
            DetectorState::NotRun(ScanStop::InvalidLimits)
        );
    }
}

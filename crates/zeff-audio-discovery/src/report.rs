#[cfg(not(target_arch = "wasm32"))]
use super::cdda;
use super::{
    MAX_CANDIDATES, MAX_ROM_BYTES, MAX_SCAN_WORK, SongCandidate, detectors, gax, gb_music, natsume,
    nes_music, rips, tables, tracker, vgm,
};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ScanLimits {
    pub max_work: u64,
    pub max_candidates: u32,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_work: super::MAX_SCAN_WORK,
            max_candidates: 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScanReport {
    pub schema: &'static str,
    pub detector: &'static str,
    pub detector_version: u32,
    pub applicable_detectors: &'static [detectors::DetectorDescriptor],
    pub limitations: &'static [&'static str],
    pub media: MediaIdentity,
    pub limits: ScanLimits,
    pub work_used: u64,
    pub status: ScanStatus,
    pub detector_outcomes: Vec<DetectorOutcome>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub driver_candidates: Vec<super::drivers::DriverCandidate>,
    pub candidates: Vec<SongCandidate>,
    pub song_tables: Vec<tables::SongTableInventory>,
    pub gax_songs: Vec<gax::GaxSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub engine_software_songs: Vec<super::engine_software::EngineSoftwareSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub krawall_songs: Vec<super::krawall::KrawallSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gax_native_songs: Vec<super::gax_native::GaxNativeSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub musyx_songs: Vec<super::musyx::MusyxSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aas_songs: Vec<super::aas::AasSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub descriptor_midi_songs: Vec<super::descriptor_midi::DescriptorMidiSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nsq_songs: Vec<super::nsq::NsqSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub radriver_songs: Vec<super::radriver::RadriverSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gbass_songs: Vec<super::gbass::GbassSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aas_stream_songs: Vec<super::aas_stream::AasStreamSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aas_pcm_songs: Vec<super::aas_pcm::AasPcmSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gb_songs: Vec<gb_music::GbSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gb_native_songs: Vec<super::gb_native::GbNativeSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub huge_songs: Vec<super::huge::catalog::HugeSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gb_musyx_songs: Vec<super::gb_musyx::GbMusyxSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gb_tose_songs: Vec<super::gb_tose::GbToseSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gb_quickthunder_songs: Vec<super::gb_quickthunder::GbQuickThunderSong>,
    pub gb_ghx_songs: Vec<super::gb_ghx::GbGhxSong>,
    pub gb_sound_system_songs: Vec<super::gb_sound_system::GbSoundSystemSong>,
    pub gb_carillon_songs: Vec<super::gb_carillon::GbCarillonSong>,
    pub ws_tose_songs: Vec<super::ws_tose::WsToseSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nes_tose_songs: Vec<super::nes_tose::NesToseSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nes_songs: Vec<nes_music::NesSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nes_native_songs: Vec<super::nes_native::NesNativeSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sega_psg_songs: Vec<super::sega_psg::SegaPsgSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub natsume_songs: Vec<natsume::NatsumeSong>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub vgm_logs: Vec<vgm::VgmLog>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub music_rips: Vec<rips::MusicRip>,
    pub tracker_modules: Vec<tracker::EmbeddedModule>,
    #[cfg(not(target_arch = "wasm32"))]
    pub cdda_tracks: Vec<cdda::CdAudioTrack>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DetectorOutcome {
    pub descriptor: detectors::DetectorDescriptor,
    pub state: DetectorState,
    pub retained_matches: u32,
    pub work_used: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum DetectorState {
    Complete,
    Unsupported,
    Malformed(MalformedInput),
    Incomplete(ScanStop),
    NotRun(ScanStop),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MediaIdentity {
    pub system: &'static str,
    pub byte_len: u64,
    pub sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum ScanStatus {
    Complete,
    Unsupported,
    Malformed(MalformedInput),
    Incomplete(ScanStop),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MalformedInput {
    TruncatedHeader,
    EmptyProgram,
    InvalidSongCount,
    InvalidFirstSong,
    InvalidAddress,
    ProgramLengthExceedsSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStop {
    Cancelled,
    WorkLimit,
    CandidateLimit,
    MediaLimit,
    InvalidLimits,
    InventoryLimit,
    ValidationLimit,
}

impl ScanReport {
    pub fn new(
        detector: &'static str,
        detector_version: u32,
        applicable_detectors: &'static [detectors::DetectorDescriptor],
        limitations: &'static [&'static str],
        media: MediaIdentity,
        limits: ScanLimits,
    ) -> Self {
        Self {
            schema: "zeff-audio-discovery/1",
            detector,
            detector_version,
            applicable_detectors,
            limitations,
            media,
            limits,
            work_used: 0,
            status: ScanStatus::Complete,
            detector_outcomes: Vec::new(),
            driver_candidates: Vec::new(),
            candidates: Vec::new(),
            song_tables: Vec::new(),
            gax_songs: Vec::new(),
            engine_software_songs: Vec::new(),
            krawall_songs: Vec::new(),
            gax_native_songs: Vec::new(),
            musyx_songs: Vec::new(),
            aas_songs: Vec::new(),
            descriptor_midi_songs: Vec::new(),
            nsq_songs: Vec::new(),
            radriver_songs: Vec::new(),
            gbass_songs: Vec::new(),
            aas_stream_songs: Vec::new(),
            aas_pcm_songs: Vec::new(),
            gb_songs: Vec::new(),
            gb_native_songs: Vec::new(),
            huge_songs: Vec::new(),
            gb_musyx_songs: Vec::new(),
            gb_tose_songs: Vec::new(),
            gb_quickthunder_songs: Vec::new(),
            gb_ghx_songs: Vec::new(),
            gb_sound_system_songs: Vec::new(),
            gb_carillon_songs: Vec::new(),
            ws_tose_songs: Vec::new(),
            nes_tose_songs: Vec::new(),
            nes_songs: Vec::new(),
            nes_native_songs: Vec::new(),
            sega_psg_songs: Vec::new(),
            natsume_songs: Vec::new(),
            vgm_logs: Vec::new(),
            music_rips: Vec::new(),
            tracker_modules: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            cdda_tracks: Vec::new(),
        }
    }

    pub fn preflight(&mut self, cancel: &AtomicBool) -> Option<ScanStop> {
        self.preflight_with_media_limit(cancel, MAX_ROM_BYTES as u64)
    }

    pub fn preflight_with_media_limit(
        &mut self,
        cancel: &AtomicBool,
        max_bytes: u64,
    ) -> Option<ScanStop> {
        let reason = if cancel.load(Ordering::Relaxed) {
            Some(ScanStop::Cancelled)
        } else if self.media.byte_len > max_bytes {
            Some(ScanStop::MediaLimit)
        } else if self.limits.max_work > MAX_SCAN_WORK
            || self.limits.max_candidates > MAX_CANDIDATES
        {
            Some(ScanStop::InvalidLimits)
        } else {
            None
        };
        if let Some(reason) = reason {
            self.status = ScanStatus::Incomplete(reason);
            self.finish_not_run(reason);
        }
        reason
    }

    /// Construction helper: record each applicable descriptor at most once.
    pub fn record_detector(
        &mut self,
        descriptor: detectors::DetectorDescriptor,
        state: DetectorState,
        retained_matches: usize,
        work_used: u64,
    ) {
        debug_assert!(
            !self
                .detector_outcomes
                .iter()
                .any(|outcome| outcome.descriptor == descriptor)
        );
        debug_assert!(self.applicable_detectors.contains(&descriptor));
        self.detector_outcomes.push(DetectorOutcome {
            descriptor,
            state,
            retained_matches: retained_matches as u32,
            work_used,
        });
    }

    /// Construction helper; existing outcomes must use applicable descriptors.
    pub fn finish_not_run(&mut self, reason: ScanStop) {
        for descriptor in self.applicable_detectors {
            if !self
                .detector_outcomes
                .iter()
                .any(|outcome| outcome.descriptor == *descriptor)
            {
                self.record_detector(*descriptor, DetectorState::NotRun(reason), 0, 0);
            }
        }
        self.detector_outcomes.sort_by_key(|outcome| {
            self.applicable_detectors
                .iter()
                .position(|descriptor| *descriptor == outcome.descriptor)
                .expect("outcomes always use applicable descriptors")
        });
    }
}

use crate::tas_project::{TasDigest, TasExternalIdentity};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum TasExecutionProfile {
    DirectNesCartridge,
    DirectFdsDisk,
    DirectGbCartridgeDmg,
    DirectGbCartridgeCgb,
    DirectColecoCartridge,
    DirectSmsCartridge,
    DirectGameGearCartridge,
    DirectGbaCartridge,
    DirectSg1000Cartridge,
    DirectWsCartridge,
    DirectPceHuCard,
    DirectPceSixButtonHuCard,
    DirectPceMultitapHuCard,
    DirectPceCd,
    DirectPceMultitapCd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasPersistenceBaseline {
    Missing,
    Present { byte_len: u64, sha256: TasDigest },
}

impl From<crate::save_paths::SaveTargetBaseline> for TasPersistenceBaseline {
    fn from(value: crate::save_paths::SaveTargetBaseline) -> Self {
        match value {
            crate::save_paths::SaveTargetBaseline::Missing => Self::Missing,
            crate::save_paths::SaveTargetBaseline::Present { byte_len, sha256 } => Self::Present {
                byte_len,
                sha256: TasDigest(sha256),
            },
        }
    }
}

impl From<TasPersistenceBaseline> for crate::save_paths::SaveTargetBaseline {
    fn from(value: TasPersistenceBaseline) -> Self {
        match value {
            TasPersistenceBaseline::Missing => Self::Missing,
            TasPersistenceBaseline::Present { byte_len, sha256 } => Self::Present {
                byte_len,
                sha256: sha256.0,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasGbaPersistenceKind {
    Sram,
    Flash512,
    Flash1M,
    Eeprom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasPersistenceContract {
    Absent,
    NesBattery {
        byte_len: u64,
        initial_sha256: TasDigest,
        target_baseline: TasPersistenceBaseline,
    },
    GbBattery {
        byte_len: u64,
        initial_sha256: TasDigest,
        target_baseline: TasPersistenceBaseline,
    },
    GbRtcBattery {
        persistent_state: TasExternalIdentity,
        rtc_state: TasExternalIdentity,
        byte_len: u64,
        initial_sha256: TasDigest,
        target_baseline: TasPersistenceBaseline,
    },
    GbaBattery {
        kind: TasGbaPersistenceKind,
        byte_len: u64,
        initial_sha256: TasDigest,
        target_baseline: TasPersistenceBaseline,
    },
    GbaRtcBattery {
        kind: zeff_gba_core::hardware::cartridge::BackupKind,
        persistent_state: TasExternalIdentity,
        rtc_state: TasExternalIdentity,
        byte_len: u64,
        initial_sha256: TasDigest,
        target_baseline: TasPersistenceBaseline,
    },
    GameGearBattery8KiB {
        byte_len: u64,
        initial_sha256: TasDigest,
        target_baseline: TasPersistenceBaseline,
    },
    WsBattery {
        save_kind: zeff_ws_core::hardware::cartridge::SaveKind,
        byte_len: u64,
        initial_sha256: TasDigest,
        target_baseline: TasPersistenceBaseline,
    },
    WsRtcBattery {
        save_kind: zeff_ws_core::hardware::cartridge::SaveKind,
        persistent_state: TasExternalIdentity,
        rtc_state: TasExternalIdentity,
        byte_len: u64,
        initial_sha256: TasDigest,
        target_baseline: TasPersistenceBaseline,
    },
}

#[derive(Clone, Copy)]
struct TasPersistenceMetadata {
    byte_len: u64,
    initial_sha256: TasDigest,
    target_baseline: TasPersistenceBaseline,
}

impl TasPersistenceContract {
    fn metadata(self) -> Option<TasPersistenceMetadata> {
        match self {
            Self::Absent => None,
            Self::NesBattery {
                byte_len,
                initial_sha256,
                target_baseline,
            }
            | Self::GbBattery {
                byte_len,
                initial_sha256,
                target_baseline,
            }
            | Self::GbRtcBattery {
                byte_len,
                initial_sha256,
                target_baseline,
                ..
            }
            | Self::GbaBattery {
                byte_len,
                initial_sha256,
                target_baseline,
                ..
            }
            | Self::GbaRtcBattery {
                byte_len,
                initial_sha256,
                target_baseline,
                ..
            }
            | Self::GameGearBattery8KiB {
                byte_len,
                initial_sha256,
                target_baseline,
            }
            | Self::WsBattery {
                byte_len,
                initial_sha256,
                target_baseline,
                ..
            }
            | Self::WsRtcBattery {
                byte_len,
                initial_sha256,
                target_baseline,
                ..
            } => Some(TasPersistenceMetadata {
                byte_len,
                initial_sha256,
                target_baseline,
            }),
        }
    }

    pub(crate) fn byte_len(self) -> Option<u64> {
        self.metadata().map(|metadata| metadata.byte_len)
    }

    pub(crate) fn initial_sha256(self) -> Option<TasDigest> {
        self.metadata().map(|metadata| metadata.initial_sha256)
    }

    pub(crate) fn target_baseline(self) -> Option<TasPersistenceBaseline> {
        self.metadata().map(|metadata| metadata.target_baseline)
    }

    pub(crate) fn rtc_identities(self) -> Option<(TasExternalIdentity, TasExternalIdentity)> {
        match self {
            Self::GbRtcBattery {
                persistent_state,
                rtc_state,
                ..
            }
            | Self::GbaRtcBattery {
                persistent_state,
                rtc_state,
                ..
            }
            | Self::WsRtcBattery {
                persistent_state,
                rtc_state,
                ..
            } => Some((persistent_state, rtc_state)),
            _ => None,
        }
    }

    pub(crate) fn is_rtc_battery(self) -> bool {
        self.rtc_identities().is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TasPersistencePublicationOutcome {
    NotRequired,
    NotPublished {
        error: String,
    },
    PublishedDurable {
        path: String,
        generation: u64,
        component_sha256: TasDigest,
    },
    PublishedDurabilityUncertain {
        path: Option<String>,
        error: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TasLoadedProfileObservation {
    pub(crate) profile: TasExecutionProfile,
    pub(crate) system: crate::emu_backend::ActiveSystem,
    pub(crate) identity_metadata_matches: bool,
    pub(crate) load_provenance_available: bool,
    pub(crate) direct_source: Option<bool>,
    pub(crate) source_media_sha256: Option<TasDigest>,
    pub(crate) effective_media_sha256: Option<TasDigest>,
    pub(crate) mods_absent: Option<bool>,
    pub(crate) persistent_state_absent: Option<bool>,
    pub(crate) project_owned_persistence: Option<TasPersistenceContract>,
    pub(crate) initial_input_neutral: Option<bool>,
    pub(crate) configured_at_load_sample_rate: Option<u32>,
    pub(crate) initial_sample_rate: Option<u32>,
    pub(crate) current_sample_rate: Option<u32>,
    pub(crate) firmware_profile_matches: bool,
    pub(crate) hardware_profile_matches: bool,
    pub(crate) controller_profile_matches: bool,
    pub(crate) removable_media_absent: bool,
    pub(crate) cheats_absent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TasRepairIdentity {
    pub(crate) repair_id: u64,
    pub(crate) suspension_token: u64,
    pub(crate) project_content_sha256: TasDigest,
    pub(crate) profile: TasExecutionProfile,
    pub(crate) source_media_sha256: TasDigest,
    pub(crate) effective_media_sha256: TasDigest,
    pub(crate) required_sample_rate: u32,
    pub(crate) persistence: TasPersistenceContract,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TasRepairSuspensionProof {
    pub(crate) identity: TasRepairIdentity,
    pub(crate) state_sha256: TasDigest,
    pub(crate) frame_count: u64,
    pub(crate) framebuffer_sha256: TasDigest,
    pub(crate) framebuffer_len: usize,
    pub(crate) loaded_profile: TasLoadedProfileObservation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasRepairSuspendRejectedReason {
    InvalidIdentity,
    AlreadyLeased,
    UncappedExecution,
    AudioRecordingActive,
    LinkActivity,
    PendingFrameDelivery,
    RuntimeFault,
    ReplayActivityUnwitnessed,
    ProfileMismatch,
    SourceMediaMismatch,
    EffectiveMediaMismatch,
    UnsafeLoadedProfile,
    StateCaptureFailed,
    StateChangedDuringCapture,
    FramebufferUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasRepairAction {
    ResumeOriginal,
    DiscardOriginal,
    CommitRepaired,
    DiscardRepaired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasRepairActionRejectedReason {
    NoMatchingRepair,
    StaleToken,
    SuspensionProofMismatch,
    StateCaptureFailed,
    StateDigestMismatch,
    FrameCountMismatch,
    FramebufferMismatch,
    LoadedProfileMismatch,
    TasRollbackFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasControlAcquireRejectedReason {
    AlreadyLeased { lease_id: u64 },
    UncappedExecution,
    AudioRecordingActive,
    LinkActivity,
    PendingFrameDelivery,
    RuntimeFault,
    ReplayActivityUnwitnessed,
    UnsupportedSystem,
    IdentityMetadataMismatch,
    LoadProvenanceUnavailable,
    DirectNesFileRequired,
    SourceMediaMismatch,
    ModsEnabledOrApplied,
    PersistentStateNotAbsent,
    NonNeutralInitialInput,
    NonDefaultSampleRate,
    FirmwarePresent,
    NonStandardConsoleHardware,
    NonStandardControllerTopology,
    RemovableMediaPresent,
    CheatsPresent,
    StateWitnessUnavailable,
    LeaseIdExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasControlRollbackRejectedReason {
    NoActiveLease,
    WrongLease { active_lease_id: u64 },
    RestoreFailed,
    StateVerificationUnavailable,
    StateDigestMismatch,
    FrameCountMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasControlCommitRejectedReason {
    NoActiveLease,
    WrongLease { active_lease_id: u64 },
    NoCompletedExecution,
    StateVerificationUnavailable,
    CandidateStateDigestMismatch,
    CandidateFrameCountMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasExecutionRejectedReason {
    NoActiveLease,
    WrongLease { active_lease_id: u64 },
    WrongExecutionProfile { active_profile: TasExecutionProfile },
    InvalidRunId,
    RunAlreadyAttempted { active_run_id: u64 },
    InvalidCacheProof,
    FrameLimitExceeded,
    StartStateTooLarge,
    InvalidStartState,
    StartStateRestoreFailed,
    NonStandardControllerTopology,
    InvalidInput,
    FrameCountOverflow,
    FrameProgressFailed,
    RuntimeFault,
    StateCaptureFailed,
    StateFrameMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasFrameAdvanceRejectedReason {
    NoActiveLease,
    WrongLease { active_lease_id: u64 },
    WrongExecutionProfile { active_profile: TasExecutionProfile },
    NoCompletedExecution,
    WrongRun { active_run_id: u64 },
    InvalidAdvanceId,
    UnexpectedAdvanceId { expected_advance_id: u64 },
    AdvanceIdExhausted,
    UnexpectedSegmentId { expected_segment_id: u64 },
    SegmentIdExhausted,
    SegmentProofMismatch,
    CandidateProofMismatch,
    FrameLimitExceeded,
    StateVerificationUnavailable,
    CandidateStateDigestMismatch,
    CandidateFrameCountMismatch,
    FrameCountOverflow,
    FrameProgressFailed,
    RuntimeFault,
    StateCaptureFailed,
    StateFrameMismatch,
    InvalidInput,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TasInputFrame {
    pub(crate) p1_buttons: u8,
    pub(crate) p1_dpad: u8,
    pub(crate) p2_buttons: u8,
    pub(crate) p2_dpad: u8,
    pub(crate) p3_buttons: u8,
    pub(crate) p3_dpad: u8,
    pub(crate) p4_buttons: u8,
    pub(crate) p4_dpad: u8,
    pub(crate) p5_buttons: u8,
    pub(crate) p5_dpad: u8,
    pub(crate) coleco: [crate::tas_project::TasColecoControllerInput; 2],
    pub(crate) zapper: zeff_emu_common::replay::ReplayZapperFrame,
    pub(crate) tilt_x_bits: u32,
    pub(crate) tilt_y_bits: u32,
    pub(crate) fds_disk_side: Option<u8>,
    pub(crate) fds_write_protected: Option<bool>,
    pub(crate) fds_media_event: Option<TasFdsMediaEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasFdsMediaEvent {
    Eject,
    Insert { side: u8, write_protected: bool },
}

pub(crate) struct TasExecutionRequest {
    pub(crate) profile: TasExecutionProfile,
    pub(crate) lease_id: u64,
    pub(crate) run_id: u64,
    pub(crate) cache_proof: TasExecutionCacheProof,
    pub(crate) intermediate_cache_proofs: Vec<TasExecutionCacheProof>,
    pub(crate) predecessor_window: Option<TasExecutionPredecessorWindow>,
    pub(crate) start_state_bytes: Vec<u8>,
    pub(crate) input_prefix: Vec<TasInputFrame>,
}

pub(crate) struct TasExecutionPredecessorWindow {
    pub(crate) source_proofs: Vec<TasExecutionCacheProof>,
    pub(crate) input_start_cursor: u64,
    pub(crate) input_frames: Vec<TasInputFrame>,
}

pub(crate) fn tas_intermediate_cache_cursors(target_cursor: u64) -> Vec<u64> {
    const HOT_COUNT: usize = 8;
    const SPARSE_COUNT: usize = 7;
    const HOT_STEP: u64 = crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES;
    const SPARSE_STEP: u64 = HOT_STEP * 8;

    let mut cursors = Vec::with_capacity(HOT_COUNT + SPARSE_COUNT);
    let mut cursor = target_cursor.saturating_sub(1) / HOT_STEP * HOT_STEP;
    for _ in 0..HOT_COUNT {
        if cursor == 0 {
            break;
        }
        cursors.push(cursor);
        cursor = cursor.saturating_sub(HOT_STEP);
    }
    cursor = cursor / SPARSE_STEP * SPARSE_STEP;
    for _ in 0..SPARSE_COUNT {
        if cursor == 0 {
            break;
        }
        cursors.push(cursor);
        cursor = cursor.saturating_sub(SPARSE_STEP);
    }
    cursors.sort_unstable();
    cursors.dedup();
    cursors
}

pub(crate) fn tas_is_intermediate_cache_cursor(target_cursor: u64, cursor: u64) -> bool {
    const HOT_STEP: u64 = crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES;
    let mut candidate = target_cursor.saturating_sub(1) / HOT_STEP * HOT_STEP;
    for _ in 0..8 {
        if candidate == 0 {
            return false;
        }
        if candidate == cursor {
            return true;
        }
        candidate = candidate.saturating_sub(HOT_STEP);
    }
    candidate = candidate / (HOT_STEP * 8) * (HOT_STEP * 8);
    for _ in 0..7 {
        if candidate == 0 {
            return false;
        }
        if candidate == cursor {
            return true;
        }
        candidate = candidate.saturating_sub(HOT_STEP * 8);
    }
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TasExecutionCacheProof {
    pub(crate) sync_identity_sha256: TasDigest,
    pub(crate) branch_prefix_sha256: TasDigest,
    pub(crate) target_cursor: u64,
}

pub(crate) struct TasFrameAdvanceRequest {
    pub(crate) profile: TasExecutionProfile,
    pub(crate) lease_id: u64,
    pub(crate) run_id: u64,
    pub(crate) advance_id: u64,
    pub(crate) segment_id: u64,
    pub(crate) expected_segment_frame_count: u64,
    pub(crate) expected_executed_project_frames: u64,
    pub(crate) expected_frame_count: u64,
    pub(crate) expected_state_sha256: TasDigest,
    pub(crate) input: TasInputFrame,
    pub(crate) snapshot: Option<TasFrameAdvanceSnapshot>,
}

pub(crate) struct TasFrameAdvanceSnapshot {
    pub(crate) request: super::SnapshotRequest,
    pub(crate) buffers: super::ReusableBuffers,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TasControlLeaseWitness {
    pub(crate) profile: TasExecutionProfile,
    pub(crate) frame_count: u64,
    pub(crate) source_media_sha256: TasDigest,
    pub(crate) effective_media_sha256: TasDigest,
    pub(crate) current_state_bytes: Vec<u8>,
    pub(crate) current_state_sha256: TasDigest,
    pub(crate) determinism_abi: &'static str,
    pub(crate) state_format_compatibility_id: &'static str,
    pub(crate) sync_config_sha256: TasDigest,
}

#[cfg(test)]
mod tests {
    use super::{tas_intermediate_cache_cursors, tas_is_intermediate_cache_cursor};

    #[test]
    fn intermediate_cache_plan_is_bounded_tiered_and_shared_by_nearby_targets() {
        let cursors = tas_intermediate_cache_cursors(100_000);
        assert!(cursors.len() <= 15);
        assert!(cursors.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            cursors
                .iter()
                .all(|cursor| tas_is_intermediate_cache_cursor(100_000, *cursor))
        );
        assert!(cursors.contains(&99_600));
        assert!(tas_intermediate_cache_cursors(100_001).contains(&99_600));
        assert!(cursors.windows(2).any(|pair| pair[1] - pair[0] == 4_800));
    }
}

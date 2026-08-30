use crate::tas_project::TasDigest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasExecutionProfile {
    DirectNesCartridge,
    DirectGbRomOnlyDmg,
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
    EmptyInputPrefix,
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
}

#[derive(Clone)]
pub(crate) struct TasExecutionRequest {
    pub(crate) profile: TasExecutionProfile,
    pub(crate) lease_id: u64,
    pub(crate) run_id: u64,
    pub(crate) start_state_bytes: Vec<u8>,
    pub(crate) input_prefix: Vec<TasInputFrame>,
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

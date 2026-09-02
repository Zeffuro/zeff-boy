mod commands;
#[cfg(not(target_arch = "wasm32"))]
mod emu_loop;
#[cfg(not(target_arch = "wasm32"))]
mod persistence;
mod recovery;
#[cfg(not(target_arch = "wasm32"))]
mod runner;
mod shared;
mod speculation;
#[cfg(not(target_arch = "wasm32"))]
mod state;
mod types;

#[cfg(test)]
mod contract_tests;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{
    EmuResponsePoll, EmuThread, SuspendedEmuThread, TasRepairReleaseFailure,
    TasRepairSuspendFailure,
};
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use recovery::RecoveryTestConfig;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use recovery::inspect_freshness_for_test;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::EmuThread;

#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use types::TasPersistenceBaseline;
#[cfg(feature = "profile-cores")]
pub(crate) use types::profile_frame_publication;
pub(crate) use types::{
    AudioConfig, AudioRecordingCapture, EmuCommand, EmuCommandAuthority, EmuResponse, FrameInput,
    FrameResult, GuestCallRequest, JoypadInput, MemorySearchRequest, PceMouseInput, RenderSettings,
    ReplayJoypadFrame, ReplayStartState, ReusableBuffers, SharedFramebuffer, SnapshotRequest,
    SpeculationBlockers, TasControlCommandKind, WorkerRuntimeFault, ZapperInput,
};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use types::{
    TasControlAcquireRejectedReason, TasControlCommitRejectedReason, TasControlLeaseWitness,
    TasControlRollbackRejectedReason, TasExecutionCacheProof, TasExecutionPredecessorWindow,
    TasExecutionProfile, TasExecutionRejectedReason, TasExecutionRequest, TasFdsMediaEvent,
    TasFrameAdvanceRejectedReason, TasFrameAdvanceRequest, TasFrameAdvanceSnapshot,
    TasGbaPersistenceKind, TasInputFrame, TasLoadedProfileObservation, TasPersistenceContract,
    TasPersistencePublicationOutcome, TasRepairAction, TasRepairActionRejectedReason,
    TasRepairIdentity, TasRepairSuspendRejectedReason, TasRepairSuspensionProof, TcpLinkMode,
    tas_intermediate_cache_cursors, tas_is_intermediate_cache_cursor,
};

pub(crate) const DEFAULT_REWIND_SECONDS: usize = 10;
pub(crate) const REWIND_CAPTURE_INTERVAL_FRAMES: usize = 4;
pub(crate) const DEFAULT_UNCAPPED_BATCH_SIZE: usize = 60;
pub(crate) const MAX_UNCAPPED_BATCH_SIZE: usize = 240;

#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) fn build_tas_repair_witness(
    backend: &crate::emu_backend::EmuBackend,
    profile: TasExecutionProfile,
) -> Result<TasControlLeaseWitness, TasControlAcquireRejectedReason> {
    emu_loop::tas_control::witness::build_tas_witness(backend, false, profile)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn build_tas_repair_witness_for_persistence(
    backend: &crate::emu_backend::EmuBackend,
    profile: TasExecutionProfile,
    persistence: TasPersistenceContract,
) -> Result<TasControlLeaseWitness, TasControlAcquireRejectedReason> {
    emu_loop::tas_control::witness::build_tas_witness_for_persistence(
        backend,
        false,
        profile,
        persistence,
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn observe_tas_repair_profile(
    backend: &crate::emu_backend::EmuBackend,
    profile: TasExecutionProfile,
) -> TasLoadedProfileObservation {
    emu_loop::tas_control::witness::observe_loaded_profile(backend, false, profile)
}

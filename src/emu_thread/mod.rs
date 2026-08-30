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
pub(crate) use native::{EmuResponsePoll, EmuThread};

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::EmuThread;

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
    TasControlRollbackRejectedReason, TasExecutionProfile, TasExecutionRejectedReason,
    TasExecutionRequest, TasFrameAdvanceRejectedReason, TasFrameAdvanceRequest, TasInputFrame,
    TcpLinkMode,
};

pub(crate) const DEFAULT_REWIND_SECONDS: usize = 10;
pub(crate) const REWIND_CAPTURE_INTERVAL_FRAMES: usize = 4;
pub(crate) const DEFAULT_UNCAPPED_BATCH_SIZE: usize = 60;
pub(crate) const MAX_UNCAPPED_BATCH_SIZE: usize = 240;

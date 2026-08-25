#[cfg(not(target_arch = "wasm32"))]
mod emu_loop;
#[cfg(not(target_arch = "wasm32"))]
mod runner;
mod shared;
#[cfg(not(target_arch = "wasm32"))]
mod state;
mod types;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::EmuThread;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::EmuThread;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use types::TcpLinkMode;
#[cfg(feature = "profile-cores")]
pub(crate) use types::profile_frame_publication;
pub(crate) use types::{
    AudioConfig, AudioRecordingCapture, EmuCommand, EmuResponse, FrameInput, FrameResult,
    GuestCallRequest, JoypadInput, MemorySearchRequest, PceMouseInput, RenderSettings,
    ReplayJoypadFrame, ReplayStartState, ReusableBuffers, SharedFramebuffer, SnapshotRequest,
    WorkerRuntimeFault, ZapperInput,
};

pub(crate) const DEFAULT_REWIND_SECONDS: usize = 10;
pub(crate) const REWIND_CAPTURE_INTERVAL_FRAMES: usize = 4;
pub(crate) const DEFAULT_UNCAPPED_BATCH_SIZE: usize = 60;
pub(crate) const MAX_UNCAPPED_BATCH_SIZE: usize = 240;

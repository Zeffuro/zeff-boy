mod cheats;
#[cfg(not(target_arch = "wasm32"))]
mod debug_actions;
#[cfg(not(target_arch = "wasm32"))]
mod emu_loop;
#[cfg(not(target_arch = "wasm32"))]
mod runner;
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

pub(crate) use types::{
    AudioConfig, EmuCommand, EmuResponse, FrameInput, FrameResult, JoypadInput,
    MemorySearchRequest, RenderSettings, ReusableBuffers, SharedFramebuffer, SnapshotRequest,
};

pub(crate) const DEFAULT_REWIND_SECONDS: usize = 10;
pub(crate) const REWIND_SNAPSHOTS_PER_SECOND: usize = 4;

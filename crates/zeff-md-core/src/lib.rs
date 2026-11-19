#![forbid(unsafe_code)]

mod cartridge;
pub mod constants;
#[cfg(feature = "first-failure-diagnostic")]
pub mod diagnostic;
pub mod emulator;
mod hardware;
#[cfg(feature = "ram-copy-census")]
pub mod ram_census;
mod save_state;
pub mod sound_trace;

pub use cartridge::{CartridgeHeader, LoadError};
pub use emulator::MAX_FRONTEND_STATE_BYTES;
pub use emulator::cheats;
pub use emulator::{
    DebugCallContext, Emulator, GuestCallError, InputMut, LoadConfig, MachineFault, MachineStep,
    Region, TimingModel, VdpCaptureError,
};
pub use emulator::{WarmResetError, WarmResetFailure, WarmResetRejection, WarmResetReport};
pub use hardware::audio::{
    MD_AUDIO_REPLAY_MAX_FRAMES, MdAudioReplay, Opn2, Opn2DebugSnapshot, Psg, PsgDebugSnapshot,
};
pub use hardware::bus::BusError;
pub use hardware::input::{ControllerKind, ControllerPort, Input, InputChange, InputFault};
pub use hardware::sound::SoundFault;
pub use hardware::vdp::{
    ControlCommand, ControlWrite, Vdp, VdpControl, VdpDmaCopy, VdpDmaError, VdpDmaRegisters,
    VdpDmaSource, VdpDmaWrite, VdpEvent, VdpH32Timing, VdpHBlankEdge, VdpInterruptClock,
    VdpInterruptEdge, VdpInterruptError, VdpInterrupts, VdpLineError, VdpMemory, VdpPixel,
    VdpPlane, VdpPortError, VdpReadCache, VdpReadError, VdpRegisterError, VdpRegisterPipeline,
    VdpRegisterWrite, VdpRenderError, VdpScanline, VdpScheduleError, VdpSprite, VdpSpriteCache,
    VdpSpriteLine, VdpSpritePattern, VdpSpriteRow, VdpStatusError, VdpTileRow, VdpTimingError,
    VdpVBlankEdge, VdpVerticalTiming, VdpWrite, VdpWriteError, VdpWriteFifo,
};

#[path = "cartridge/svp/mod.rs"]
pub mod svp;

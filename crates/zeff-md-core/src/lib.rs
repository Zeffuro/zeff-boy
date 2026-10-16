#![forbid(unsafe_code)]

mod cartridge;
pub mod constants;
pub mod emulator;
mod hardware;
mod save_state;

pub use cartridge::{CartridgeHeader, LoadError};
pub use emulator::{Emulator, LoadConfig, MachineFault, MachineStep, Region, VdpCaptureError};
pub use hardware::audio::{Opn2, Opn2DebugSnapshot, Psg, PsgDebugSnapshot};
pub use hardware::bus::BusError;
pub use hardware::input::{ControllerKind, ControllerPort, Input};
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

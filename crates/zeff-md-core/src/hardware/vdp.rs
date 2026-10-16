mod constants;
mod device;
mod dma;
mod dma_source;
pub use dma_source::VdpDmaSource;
mod fifo;
mod interrupts;
mod memory;
mod mode;
mod read;
mod registers;
mod render;
mod sprite_cache;
mod sprites;
mod state;
mod timing;
pub(crate) use timing::FieldPosition;

use constants::*;
pub use device::{Vdp, VdpLineError, VdpPortError, VdpScanline, VdpStatusError};
pub use dma::{VdpDmaCopy, VdpDmaError, VdpDmaRegisters, VdpDmaWrite};
pub use fifo::VdpWriteFifo;
pub(crate) use interrupts::SoundInterrupt;
pub use interrupts::{VdpEvent, VdpInterruptError, VdpInterrupts, VdpScheduleError};
pub use memory::VdpMemory;
pub use read::{VdpReadCache, VdpReadError};
pub use registers::{VdpRegisterError, VdpRegisterPipeline, VdpRegisterWrite};
pub use render::{VdpPixel, VdpPlane, VdpRenderError, VdpTileRow};
pub use sprite_cache::{VdpSpriteCache, VdpSpriteRow};
pub use sprites::{VdpSprite, VdpSpriteLine, VdpSpritePattern};
pub use timing::{
    VdpH32Timing, VdpHBlankEdge, VdpInterruptClock, VdpInterruptEdge, VdpTimingError,
    VdpVBlankEdge, VdpVerticalTiming,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlCommand {
    pub address: u32,
    pub code: u8,
}

impl ControlCommand {
    fn is_unused_write_target(self) -> bool {
        matches!(self.code & !COMMAND_CD4, 2 | 6 | 7 | 9..=0xb | 0xd..=0xf)
    }

    fn next_address(self, increment: u8) -> u32 {
        (self.address + u32::from(increment)) & COMMAND_ADDRESS_MASK
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlWrite {
    FirstHalf { address_low: u16, code_low: u8 },
    Register { index: u8, value: u8 },
    Command(ControlCommand),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpWrite {
    pub command: ControlCommand,
    pub data: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpWriteError {
    Full,
    InvalidCode(u8),
    InvalidAddress(u32),
    UnknownIncrement,
    UnknownSpriteTable,
    AddressOverflow,
    UnsupportedCommand(ControlCommand),
}

/// Decodes mode-5 control requests; execution and bus timing are separate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VdpControl {
    first_word: Option<u16>,
}

impl VdpControl {
    /// Applies an accepted, collision-free word to a known settled mode-5 image.
    /// Keep that image across pending words; reconcile raw writes before mixing APIs.
    /// Excludes active DMA and increment-carry aftermath; returns the raw, ungated request.
    pub fn complete_control_write(
        &mut self,
        command: &mut ControlCommand,
        word: u16,
        dma_enabled: bool,
    ) -> Result<ControlWrite, VdpWriteError> {
        if command.code & !COMMAND_CODE_MASK != 0 {
            return Err(VdpWriteError::InvalidCode(command.code));
        }
        if command.address & !COMMAND_ADDRESS_MASK != 0 {
            return Err(VdpWriteError::InvalidAddress(command.address));
        }
        let request = self.write_control(word);
        match request {
            ControlWrite::Command(mut decoded) => {
                if !dma_enabled {
                    decoded.code = (decoded.code & !COMMAND_CD5) | (command.code & COMMAND_CD5);
                }
                *command = decoded;
            }
            ControlWrite::FirstHalf { .. } | ControlWrite::Register { .. } => {
                command.address = (command.address & !u32::from(FIRST_ADDRESS_MASK))
                    | u32::from(word & FIRST_ADDRESS_MASK);
                command.code = (command.code & !FIRST_CODE_MASK)
                    | (word >> FIRST_CODE_SHIFT) as u8
                    | COMMAND_CD4;
            }
        }
        Ok(request)
    }

    pub fn write_control(&mut self, word: u16) -> ControlWrite {
        if let Some(first) = self.first_word.take() {
            return ControlWrite::Command(ControlCommand {
                address: u32::from(first & FIRST_ADDRESS_MASK)
                    | (u32::from(word & SECOND_ADDRESS_MASK) << SECOND_ADDRESS_SHIFT),
                code: ((first >> FIRST_CODE_SHIFT)
                    | ((word & SECOND_CODE_MASK) >> SECOND_CODE_SHIFT)) as u8,
            });
        }
        if word & REGISTER_TAG_MASK == REGISTER_TAG {
            return ControlWrite::Register {
                index: ((word >> REGISTER_INDEX_SHIFT) & REGISTER_INDEX_MASK) as u8,
                value: word as u8,
            };
        }
        self.first_word = Some(word);
        ControlWrite::FirstHalf {
            address_low: word & FIRST_ADDRESS_MASK,
            code_low: (word >> FIRST_CODE_SHIFT) as u8,
        }
    }

    pub fn pending_word(&self) -> Option<u16> {
        self.first_word
    }

    /// Data and status accesses break an incomplete command.
    pub fn cancel_pending(&mut self) {
        self.first_word = None;
    }
}

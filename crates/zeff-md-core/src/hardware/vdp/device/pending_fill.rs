use super::*;
use crate::hardware::vdp::{VdpDmaError, VdpDmaRegisters};

impl State {
    pub(super) fn zero_color_fill_seed(&self) -> bool {
        self.dma == Some(dma::Phase::FillWaiting)
            && self.command.code & !(COMMAND_CD4 | COMMAND_CD5) == VRAM_WRITE_CODE
            && self.fifo.len() == 1
            && self.fifo.known_zero_history()
            && self.fifo.front().is_some_and(|entry| {
                matches!(
                    entry.command.code & !COMMAND_CD4,
                    CRAM_WRITE_CODE | VSRAM_WRITE_CODE
                )
            })
            && self.read_phase.is_none()
            && !self.cache.is_ready()
            && self.control.pending_word().is_none()
            && self.registers.pending().is_none()
    }

    pub(super) fn editable_waiting_fill(&self) -> bool {
        if self.dma != Some(dma::Phase::FillWaiting)
            || !self.fifo.is_empty()
            || self.read_phase.is_some()
            || self.cache.is_ready()
            || self.interrupts.staged_blank()
            || !matches!(self.command.code, 0x21 | 0x31 | 0x32)
        {
            return false;
        }
        let published = self.registers.published_state().1;
        published.registers()[usize::from(MODE_REGISTER)].is_some_and(|mode| {
            mode & (MODE5_ENABLE | DMA_ENABLE | VRAM_128K_ENABLE) == MODE5_ENABLE | DMA_ENABLE
        }) && published.auto_increment().is_some()
            && VdpDmaRegisters::from_registers(&published)
                .is_ok_and(|registers| registers.source_high() & DMA_MODE_MASK == DMA_FILL_MODE)
    }

    fn waiting_register_value(&self, index: u8, value: u8) -> bool {
        let Some(previous) = self
            .registers
            .registers()
            .get(usize::from(index))
            .copied()
            .flatten()
        else {
            return false;
        };
        !self.registers.changes_memory_mode(index, value)
            && (index != MODE_REGISTER || value & DMA_ENABLE != 0)
            && (index != DISPLAY_SIZE_REGISTER || (previous ^ value) & INTERLACE_MODE_MASK == 0)
            && (index != DMA_SOURCE_REGISTER || value & DMA_MODE_MASK == DMA_FILL_MODE)
    }

    pub(super) fn waiting_register_write(&self, index: u8, value: u8) -> bool {
        self.waiting_register_value(index, value)
            && self.editable_waiting_fill()
            && self.control.pending_word().is_none()
            && self.registers.pending().is_none()
    }

    pub(super) fn admit_waiting_control(&self, word: u16) -> Result<bool, VdpPortError> {
        if self.dma.is_none() {
            return Ok(false);
        }
        if !self.editable_waiting_fill() || self.registers.pending().is_some() {
            return Err(VdpPortError::Dma(VdpDmaError::Busy));
        }
        let mut control = self.control;
        let mut command = self.command;
        let request = control
            .complete_control_write(&mut command, word, true)
            .map_err(VdpPortError::Control)?;
        let admitted = match request {
            crate::hardware::vdp::ControlWrite::Register { index, value } => {
                self.waiting_register_write(index, value)
            }
            crate::hardware::vdp::ControlWrite::FirstHalf { code_low, .. } => code_low == 1,
            crate::hardware::vdp::ControlWrite::Command(_) => {
                matches!(command.code, 0x01 | 0x11 | 0x21 | 0x31)
            }
        };
        if admitted {
            Ok(true)
        } else {
            Err(VdpPortError::Dma(VdpDmaError::Busy))
        }
    }

    pub(super) fn validate_waiting_retarget(&self, region: Region) -> Result<(), VdpPortError> {
        self.validate_transfer_mode(region)?;
        if !self.editable_waiting_fill() || !matches!(self.command.code, 0x21 | 0x31) {
            return Err(VdpPortError::Dma(VdpDmaError::PendingInputs));
        }
        self.validate_dma_phase(self.command, DMA_FILL_MODE, region)
    }

    pub(super) fn valid_editable_waiting_inputs(&self) -> bool {
        if !self.editable_waiting_fill() {
            return false;
        }
        match (self.registers.pending(), self.control.pending_word()) {
            (None, None) => true,
            (Some(write), None) => self.waiting_register_value(write.index, write.value),
            (None, Some(word)) => self.command.code == 0x31 && word >> FIRST_CODE_SHIFT == 1,
            _ => false,
        }
    }
}

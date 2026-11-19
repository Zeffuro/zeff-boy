use super::*;
use crate::hardware::vdp::{ControlWrite, VdpWriteError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpPortError {
    Schedule(VdpScheduleError),
    Control(VdpWriteError),
    UnsupportedDma(ControlCommand),
    UnsupportedDmaSource(u8),
    Data(VdpWriteError),
    Read(super::super::VdpReadError),
    Status(VdpStatusError),
    Dma(super::super::VdpDmaError),
}

impl State {
    pub fn control(&self) -> &VdpControl {
        &self.control
    }

    pub fn command(&self) -> ControlCommand {
        self.command
    }

    pub fn write_control(
        &mut self,
        word: u16,
        region: Region,
        memory: &super::super::VdpMemory,
    ) -> Result<ControlWrite, VdpPortError> {
        let waiting = self.admit_waiting_control(word)?;
        if self.interrupts.staged_blank()
            && !self.interrupts.blank_history().is_some_and(|b| {
                b.port.is_some_and(|p| {
                    p.kind == super::super::timing::blank_mode::PortKind::Control
                        && p.end == self.now()
                })
            })
        {
            return Err(VdpPortError::Status(VdpStatusError::UnknownBusHistory));
        }
        if !self.interrupts.staged_blank() {
            self.interrupts
                .timing_next_edge(region, &self.registers)
                .map_err(VdpPortError::Schedule)?;
        }
        if self.read_phase == Some(true) && self.control.pending_word().is_some() {
            return Err(VdpPortError::Read(
                super::super::VdpReadError::PrefetchPending,
            ));
        }
        if self.read_phase.is_some()
            && word & REGISTER_TAG_MASK == REGISTER_TAG
            && (word >> REGISTER_INDEX_SHIFT) & REGISTER_INDEX_MASK
                == u16::from(DISPLAY_SIZE_REGISTER)
            && self
                .registers
                .changes_memory_mode(DISPLAY_SIZE_REGISTER, word as u8)
            && ((word as u8) & H40_CLOCK_MASK == 1
                || self.registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
                    .is_some_and(|old| old & H40_CLOCK_MASK == 1)
                || self.interrupts.has_timing_history())
        {
            #[cfg(feature = "first-failure-diagnostic")]
            self.record_rejection(
                crate::diagnostic::RejectionSite::PortReadClock,
                DISPLAY_SIZE_REGISTER,
                word as u8,
            );
            return Err(VdpPortError::Schedule(
                VdpRegisterError::UnsupportedModeChange {
                    index: DISPLAY_SIZE_REGISTER,
                    value: word as u8,
                }
                .into(),
            ));
        }
        let mut candidate = self.clone();
        let timed = candidate.interrupts.blank_history().is_some_and(|b| {
            b.port.is_some_and(|p| {
                p.kind == super::super::timing::blank_mode::PortKind::Control
                    && p.end == candidate.now()
            })
        });
        if !timed && let Some(mut blank) = candidate.interrupts.blank_history() {
            blank.retire_port();
            candidate.interrupts.set_blank_history(Some(blank));
        }
        candidate.cache.invalidate();
        candidate.read_phase = None;
        let dma_enabled = candidate.registers.registers()[usize::from(MODE_REGISTER)]
            .expect("validated VDP mode image")
            & DMA_ENABLE
            != 0;
        let request = candidate
            .control
            .complete_control_write(&mut candidate.command, word, dma_enabled)
            .map_err(VdpPortError::Control)?;
        if timed && let Some(mut blank) = candidate.interrupts.blank_history() {
            blank.complete_control(candidate.now()).map_err(|error| {
                VdpPortError::Schedule(super::super::VdpInterruptError::from(error).into())
            })?;
            candidate.interrupts.set_blank_history(Some(blank));
        }
        match request {
            // Selectors 24..31 have no register storage or publication event.
            ControlWrite::Register { index: 24..=31, .. }
                if candidate.registers.pending().is_none() => {}
            ControlWrite::Register { index: 11..=23, .. }
                if candidate.registers.registers()[1].is_some_and(|r| r & MODE5_ENABLE == 0)
                    && candidate.registers.pending().is_none() => {}
            ControlWrite::Register { index, value } => {
                candidate
                    .queue_control_register(index, value, region)
                    .map_err(VdpPortError::Schedule)?;
            }
            ControlWrite::Command(_) if waiting && candidate.command.code & COMMAND_CD5 == 0 => {
                candidate.validate_transfer_mode(region)?;
                candidate.dma = None;
            }
            ControlWrite::Command(_) if candidate.command.code & COMMAND_CD5 != 0 => {
                if waiting {
                    candidate.validate_waiting_retarget(region)?;
                } else {
                    candidate.arm_dma(region)?;
                }
            }
            ControlWrite::Command(_)
                if matches!(
                    candidate.command.code,
                    VRAM_READ_CODE | CRAM_READ_CODE | VSRAM_READ_CODE
                ) =>
            {
                candidate.read_phase = Some(false);
                candidate.prefetch_end(region, memory)?;
            }
            ControlWrite::Command(_)
                if !matches!(
                    candidate.command.code & !COMMAND_CD4,
                    VRAM_WRITE_CODE | CRAM_WRITE_CODE | VSRAM_WRITE_CODE
                ) && !candidate.command.is_no_effect_write_target()
                    && candidate.command.code & COMMAND_CD4 == 0 =>
            {
                return Err(VdpPortError::Control(VdpWriteError::UnsupportedCommand(
                    candidate.command,
                )));
            }
            _ => {}
        }
        *self = candidate;
        Ok(request)
    }
}

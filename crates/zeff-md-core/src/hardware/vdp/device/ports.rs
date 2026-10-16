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

    /// Accepts a bus-latched control word; register effects retain their internal deadline.
    pub fn write_control(
        &mut self,
        word: u16,
        region: Region,
        memory: &super::super::VdpMemory,
    ) -> Result<ControlWrite, VdpPortError> {
        self.reject_dma_port()?;
        self.interrupts
            .timing_next_edge(region, &self.registers)
            .map_err(VdpPortError::Schedule)?;
        if self.read_phase == Some(true) {
            return Err(VdpPortError::Read(
                super::super::VdpReadError::PrefetchPending,
            ));
        }
        let mut candidate = self.clone();
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
        match request {
            ControlWrite::Register { index, value } => {
                candidate
                    .queue_control_register(index, value, region)
                    .map_err(VdpPortError::Schedule)?;
            }
            ControlWrite::Command(_) if candidate.command.code & COMMAND_CD5 != 0 => {
                candidate.arm_dma(region)?;
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
                ) && !candidate.command.is_unused_write_target() =>
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

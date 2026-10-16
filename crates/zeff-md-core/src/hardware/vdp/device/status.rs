use super::*;
use crate::hardware::vdp::{VdpInterruptError, VdpTimingError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpStatusError {
    UnknownHistory,
    ReadPending,
    UnknownBusHistory,
}

impl State {
    pub(super) fn read_cpu_status(&mut self, region: Region) -> Result<u16, VdpPortError> {
        self.validate_runtime_configuration(region)
            .map_err(VdpPortError::Schedule)?;
        let mut word = self.fifo.status_bits() | self.interrupts.status_bits();
        if region == Region::Pal {
            word |= PAL_STATUS;
        }
        if self.dma.is_some() {
            word |= DMA_BUSY_STATUS;
        }
        if self.registers.registers()[usize::from(MODE_REGISTER)].unwrap() & DISPLAY_ENABLE == 0
            || self
                .interrupts
                .vblank_flag(region, &self.registers)
                .map_err(VdpPortError::Schedule)?
        {
            word |= VBLANK_STATUS;
        }
        if self
            .interrupts
            .hblank_flag(&self.registers)
            .map_err(VdpPortError::Schedule)?
        {
            word |= HBLANK_STATUS;
        }
        self.control.cancel_pending();
        Ok(word)
    }
    pub(super) fn read_status(&mut self, region: Region) -> Result<u16, VdpPortError> {
        self.validate_transfer_mode(region)
            .map_err(|error| match error {
                VdpPortError::Schedule(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
                    VdpTimingError::UnqualifiedDisplayHistory,
                ))) => VdpPortError::Status(VdpStatusError::UnknownHistory),
                _ => error,
            })?;
        if !self.status_known {
            return Err(VdpPortError::Status(VdpStatusError::UnknownHistory));
        }
        if self.cache.is_ready() || self.read_phase.is_some() {
            return Err(VdpPortError::Status(VdpStatusError::ReadPending));
        }
        let mut word = self.fifo.status_bits() | self.interrupts.status_bits() | VBLANK_STATUS;
        if region == Region::Pal {
            word |= PAL_STATUS;
        }
        if self.dma.is_some() {
            word |= DMA_BUSY_STATUS;
        }
        if self
            .interrupts
            .hblank_flag(&self.registers)
            .map_err(VdpPortError::Schedule)?
        {
            word |= HBLANK_STATUS;
        }
        self.control.cancel_pending();
        Ok(word)
    }
}

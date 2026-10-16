use super::*;
use crate::hardware::vdp::{VdpRegisterError, VdpRegisterWrite};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpEvent {
    Register(VdpRegisterWrite),
    Interrupt(VdpInterruptEdge),
}

impl VdpEvent {
    pub fn at(self) -> MasterTicks {
        match self {
            Self::Register(write) => write.publish_at,
            Self::Interrupt(edge) => edge.at,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpScheduleError {
    Interrupt(VdpInterruptError),
    Register(VdpRegisterError),
    ClockMismatch {
        interrupt: MasterTicks,
        registers: MasterTicks,
    },
    PhaseMismatch,
    CoincidentPublication(MasterTicks),
    CoincidentAcknowledge(MasterTicks),
    EmptyAcknowledgeInterval,
    CoincidentMemory(MasterTicks),
    Read(crate::hardware::vdp::VdpReadError),
    Dma(crate::hardware::vdp::VdpDmaError),
}

impl From<VdpInterruptError> for VdpScheduleError {
    fn from(error: VdpInterruptError) -> Self {
        Self::Interrupt(error)
    }
}
impl From<VdpRegisterError> for VdpScheduleError {
    fn from(error: VdpRegisterError) -> Self {
        Self::Register(error)
    }
}

impl VdpInterrupts {
    /// Consumes one ordered event on aligned clocks with stable latched mode history.
    pub fn advance_registers_to(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &mut VdpRegisterPipeline,
    ) -> Result<Option<VdpEvent>, VdpScheduleError> {
        self.advance_pair(at, region, registers, true)
    }

    pub(crate) fn advance_pair(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &mut VdpRegisterPipeline,
        require_display: bool,
    ) -> Result<Option<VdpEvent>, VdpScheduleError> {
        if self.now != registers.now() {
            return Err(VdpScheduleError::ClockMismatch {
                interrupt: self.now,
                registers: registers.now(),
            });
        }
        let (epoch, published) = registers.published_state();
        if !self.timing.shares_internal_phase(epoch) {
            return Err(VdpScheduleError::PhaseMismatch);
        }
        if at < self.now {
            return Err(VdpInterruptError::TimeReversal {
                current: self.now,
                requested: at,
            }
            .into());
        }
        let interrupt =
            match self
                .timing
                .next_counter_edge(self.now, region, &published, require_display)
            {
                Ok(edge) => Some(edge),
                Err(VdpTimingError::ClockOverflow) => None,
                Err(error) => return Err(VdpInterruptError::from(error).into()),
            };
        let write = registers.pending().filter(|write| write.publish_at <= at);
        if let (Some(edge), Some(write)) = (interrupt, write)
            && edge.at == write.publish_at
            && require_display
        {
            return Err(VdpScheduleError::CoincidentPublication(edge.at));
        }
        let event = match (interrupt.filter(|edge| edge.at <= at), write) {
            (Some(edge), Some(write)) if write.publish_at < edge.at => {
                Some(VdpEvent::Register(write))
            }
            (Some(edge), _) => Some(VdpEvent::Interrupt(edge)),
            (None, Some(write)) => Some(VdpEvent::Register(write)),
            (None, None) => None,
        };
        let horizon = event.map_or(at, VdpEvent::at);
        let mut candidate = *self;
        let mut candidate_registers = *registers;
        let edge =
            candidate.advance_with_edge(horizon, region, &published, require_display, |_, _| {
                interrupt.ok_or(VdpTimingError::ClockOverflow)
            })?;
        let write = candidate_registers.advance_to(horizon)?;
        candidate
            .timing
            .counter_in_mode(
                horizon,
                region,
                &candidate_registers.published_state().1,
                require_display,
            )
            .map_err(VdpInterruptError::from)?;
        debug_assert_eq!(
            edge.map(VdpEvent::Interrupt)
                .or(write.map(VdpEvent::Register)),
            event
        );
        *self = candidate;
        *registers = candidate_registers;
        Ok(event)
    }

    /// Advances an isolated IACK interval, rejecting a register/interrupt edge at its completion.
    pub fn complete_scheduled_acknowledge(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &mut VdpRegisterPipeline,
    ) -> Result<u8, VdpScheduleError> {
        self.acknowledge_interval(at, region, registers, true)
    }

    pub(crate) fn acknowledge_interval(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &mut VdpRegisterPipeline,
        require_display: bool,
    ) -> Result<u8, VdpScheduleError> {
        if at == self.now {
            return Err(VdpScheduleError::EmptyAcknowledgeInterval);
        }
        let mut candidate = *self;
        let mut candidate_registers = *registers;
        while let Some(event) =
            candidate.advance_pair(at, region, &mut candidate_registers, require_display)?
        {
            if event.at() == at {
                return Err(VdpScheduleError::CoincidentAcknowledge(at));
            }
        }
        let level = candidate.complete_acknowledge(&candidate_registers)?;
        *self = candidate;
        *registers = candidate_registers;
        Ok(level)
    }
}

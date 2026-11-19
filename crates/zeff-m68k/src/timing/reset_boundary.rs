use super::PrefetchCpu;
use super::physical::{EntryKind, PhysicalEntryState, PhysicalInspection};
use crate::constants::{RESET_STATUS, TRACE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalResetBoundaryError {
    HostFrozen,
    NotOrdinaryBoundary,
    IncompleteReset,
}

impl PrefetchCpu {
    /// Pin qualification and device ownership remain the caller's responsibility.
    pub fn check_external_reset_boundary(&self) -> Result<(), ExternalResetBoundaryError> {
        if self.frozen {
            return Err(ExternalResetBoundaryError::HostFrozen);
        }
        if self.physical_state != PhysicalEntryState::Idle
            || self.physical_inspection != PhysicalInspection::DiagnosticCompleteValues
            || self.imported_boundary.is_some()
        {
            return Err(ExternalResetBoundaryError::NotOrdinaryBoundary);
        }
        Ok(())
    }

    pub fn check_external_reset_ready_boundary(&self) -> Result<(), ExternalResetBoundaryError> {
        self.check_external_reset_boundary()?;
        let ready = match (self.stopped, self.pipeline) {
            (true, None) => true,
            (false, Some(pipeline)) => pipeline.address == self.registers.pc,
            _ => false,
        };
        if self.registers.pc & 1 != 0 || !ready {
            return Err(ExternalResetBoundaryError::NotOrdinaryBoundary);
        }
        Ok(())
    }

    pub fn finish_external_reset_boundary(&mut self) -> Result<(), ExternalResetBoundaryError> {
        if self.frozen {
            return Err(ExternalResetBoundaryError::HostFrozen);
        }
        if self.physical_state != PhysicalEntryState::Critical(EntryKind::ExternalWarmReset)
            || self.physical_inspection != PhysicalInspection::DiagnosticCompleteValues
            || self.imported_boundary.is_some()
            || self.stopped
            || self.pending_trace
            || self.registers.pc & 1 != 0
            || self.registers.status_register() & (TRACE | RESET_STATUS) != RESET_STATUS
            || self
                .pipeline
                .is_none_or(|pipeline| pipeline.address != self.registers.pc)
        {
            return Err(ExternalResetBoundaryError::IncompleteReset);
        }
        self.physical_state = PhysicalEntryState::Idle;
        Ok(())
    }
}

#[cfg(test)]
mod device;
#[cfg(test)]
mod tests;

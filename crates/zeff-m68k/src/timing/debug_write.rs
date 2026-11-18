use super::physical::{PhysicalEntryState, PhysicalInspection};
use super::{ClockedBus, CpuClocks, PrefetchCpu, TimingFault};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugDataWrite {
    Byte { address: u32, value: u8 },
    Word { address: u32, value: u16 },
}

impl PrefetchCpu {
    pub fn check_debug_data_write<E>(&self) -> Result<(), TimingFault<E>> {
        self.execution_guard()?;
        if self.physical_state != PhysicalEntryState::Idle
            || self.physical_inspection != PhysicalInspection::DiagnosticCompleteValues
        {
            return Err(TimingFault::PhysicalFaultUnqualified);
        }
        if self.stopped {
            return Err(TimingFault::Stopped);
        }
        if self.pending_trace {
            return Err(TimingFault::PendingTrace);
        }
        if self.registers.pc & 1 != 0
            || self
                .pipeline
                .is_none_or(|pipe| pipe.address != self.registers.pc)
        {
            return Err(TimingFault::Unprimed);
        }
        Ok(())
    }

    // A bus failure retains elapsed clocks and any already committed device effects.
    pub fn debug_data_write<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        write: DebugDataWrite,
    ) -> Result<CpuClocks, TimingFault<B::Error>> {
        self.check_debug_data_write()?;
        let before = self.clock;
        match write {
            DebugDataWrite::Byte { address, value } => {
                self.write_data(bus, address, true, u16::from(value))?;
            }
            DebugDataWrite::Word { address, value } => {
                self.write_data(bus, address, false, value)?;
            }
        }
        Ok(CpuClocks(self.clock.0 - before.0))
    }
}

#[cfg(test)]
mod tests;

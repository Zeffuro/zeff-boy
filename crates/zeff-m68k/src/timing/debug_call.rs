use super::physical::{PhysicalEntryState, PhysicalInspection};
use super::{ClockedBus, InterruptState, PrefetchCpu, TimingFault};
use crate::TRACE;
use crate::constants::INTERRUPT_MASK;

const CALL_CONTROLS: u16 = INTERRUPT_MASK | TRACE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugCallRejection {
    Frozen,
    NonOrdinaryBoundary,
    Stopped,
    PendingTrace,
    Unprimed,
    InvalidQueue,
    OddTarget,
    OddStack,
    Level7Interrupt,
    ReturnBoundaryMismatch,
}

#[derive(Debug)]
#[must_use]
pub struct DebugCallContext {
    pc: u32,
    sp: u32,
    supervisor: bool,
    controls: u16,
    interrupts: InterruptState,
}

impl PrefetchCpu {
    pub fn check_debug_call(&self, target: u32) -> Result<(), DebugCallRejection> {
        self.check_debug_call_boundary()?;
        if target & 1 != 0 {
            return Err(DebugCallRejection::OddTarget);
        }
        Ok(())
    }

    // Bus ownership, target mapping and rollback belong to the caller.
    pub fn begin_debug_call<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        target: u32,
    ) -> Result<DebugCallContext, TimingFault<B::Error>> {
        self.check_debug_call(target)
            .map_err(TimingFault::DebugCallRejected)?;
        let context = DebugCallContext {
            pc: self.registers.pc,
            sp: self.registers.stack_pointer(),
            supervisor: self.registers.supervisor(),
            controls: self.registers.status_register() & CALL_CONTROLS,
            interrupts: self.interrupts,
        };
        self.registers
            .set_status_register((self.registers.status_register() & !TRACE) | INTERRUPT_MASK);
        self.interrupts.clear_observation();
        self.call_address(bus, target, context.pc)?;
        self.record_control_entry(
            super::ControlFlowEntryKind::Debugger,
            context.pc,
            context.pc,
            self.control_stack(
                super::ControlStackBank::active(&self.registers),
                context.sp,
                context.sp.wrapping_sub(4),
            ),
        );
        Ok(context)
    }

    pub fn debug_call_returned(&self, context: &DebugCallContext) -> bool {
        self.check_debug_call_return(context).is_ok()
    }

    pub fn finish_debug_call(
        &mut self,
        context: DebugCallContext,
    ) -> Result<(), DebugCallRejection> {
        self.check_debug_call_return(&context)?;
        self.registers.set_status_register(
            (self.registers.status_register() & !CALL_CONTROLS) | context.controls,
        );
        self.interrupts = context.interrupts;
        Ok(())
    }

    fn check_debug_call_return(
        &self,
        context: &DebugCallContext,
    ) -> Result<(), DebugCallRejection> {
        self.check_debug_call_boundary()?;
        if self.registers.pc != context.pc
            || self.registers.stack_pointer() != context.sp
            || self.registers.supervisor() != context.supervisor
        {
            return Err(DebugCallRejection::ReturnBoundaryMismatch);
        }
        Ok(())
    }

    fn check_debug_call_boundary(&self) -> Result<(), DebugCallRejection> {
        if self.frozen {
            return Err(DebugCallRejection::Frozen);
        }
        if self.physical_state != PhysicalEntryState::Idle
            || self.physical_inspection != PhysicalInspection::DiagnosticCompleteValues
            || self.imported_boundary.is_some()
        {
            return Err(DebugCallRejection::NonOrdinaryBoundary);
        }
        if self.stopped {
            return Err(DebugCallRejection::Stopped);
        }
        if self.pending_trace {
            return Err(DebugCallRejection::PendingTrace);
        }
        let pipeline = self.pipeline.ok_or(DebugCallRejection::Unprimed)?;
        if self.registers.pc & 1 != 0 || pipeline.address != self.registers.pc {
            return Err(DebugCallRejection::InvalidQueue);
        }
        if self.registers.stack_pointer() & 1 != 0 {
            return Err(DebugCallRejection::OddStack);
        }
        let (level, edge) = self.interrupts.state_parts();
        if level == 7 || edge {
            return Err(DebugCallRejection::Level7Interrupt);
        }
        if self
            .interrupts
            .observed_parts()
            .is_some_and(|(pending, level)| {
                pending && level <= ((self.registers.status_register() >> 8) & 7) as u8
            })
        {
            return Err(DebugCallRejection::NonOrdinaryBoundary);
        }
        Ok(())
    }
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod tests;

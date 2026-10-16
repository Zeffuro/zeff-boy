use super::*;
use crate::constants::{
    INTERRUPT_MASK, VECTOR_AUTOVECTOR_BASE, VECTOR_ENTRY_BYTES, VECTOR_SPURIOUS_INTERRUPT,
};
use crate::{SUPERVISOR, TRACE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptVector {
    Vector(u8),
    Autovector,
    Spurious,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InterruptState {
    level: u8,
    level7_edge: bool,
}

impl InterruptState {
    pub(super) fn state_parts(&self) -> (u8, bool) {
        (self.level, self.level7_edge)
    }

    pub(super) fn from_state_parts(level: u8, level7_edge: bool) -> Option<Self> {
        (level <= 7).then_some(Self { level, level7_edge })
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// Boundary-level input; this does not reproduce physical IPL latch latency.
    pub fn set_level(&mut self, level: u8) -> bool {
        if level > 7 {
            return false;
        }
        if level == 7 && self.level != 7 {
            self.level7_edge = true;
        }
        self.level = level;
        true
    }

    pub(super) fn accepted(&self, sr: u16) -> Option<u8> {
        if self.level7_edge {
            Some(7)
        } else if self.level > ((sr >> 8) & 7) as u8 {
            Some(self.level)
        } else {
            None
        }
    }

    pub(super) fn clear_accepted_edge(&mut self) {
        self.level7_edge = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimedOutcome {
    Instruction(TimedStep),
    Trace { clocks: u64 },
    Interrupt { level: u8, vector: u8, clocks: u64 },
    Stopped,
}

impl PrefetchCpu {
    pub fn interrupt_input(&mut self) -> &mut InterruptState {
        &mut self.interrupts
    }
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    pub fn step_boundary<B: ClockedBus>(
        &mut self,
        bus: &mut B,
    ) -> Result<TimedOutcome, TimingFault<B::Error>> {
        self.execution_guard()?;
        if let Some(clocks) = self.service_pending_trace(bus)? {
            return Ok(TimedOutcome::Trace { clocks });
        }
        if !self.stopped
            && !self
                .pipeline
                .is_some_and(|pipe| pipe.address == self.registers.pc)
        {
            return Err(TimingFault::Unprimed);
        }
        if let Some(level) = self.interrupts.accepted(self.registers.status_register()) {
            return self.interrupt(bus, level);
        }
        if self.stopped {
            return Ok(TimedOutcome::Stopped);
        }
        self.step(bus).map(TimedOutcome::Instruction)
    }

    fn interrupt<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        level: u8,
    ) -> Result<TimedOutcome, TimingFault<B::Error>> {
        let start = self.clock.0;
        let sr = self.registers.status_register();
        let pc = self.registers.pc;
        let sp = self.registers.ssp;
        self.idle(bus, 6)?;
        self.write_word(bus, sp.wrapping_sub(2), pc as u16)?;
        let access = BusAccess {
            logical_address: 0xffff_fff0 | (u32::from(level) << 1),
            bus_address: 0x00ff_fff0 | (u32::from(level) << 1),
            function_code: FunctionCode::CpuSpace,
            kind: AccessKind::InterruptAcknowledge,
            lanes: ByteLanes::Both,
        };
        self.reserve_transfer()?;
        let response = bus.acknowledge_cycle(access, self.clock);
        if matches!(response.result, Ok(BusResponse::Unsupported)) {
            self.frozen = true;
            return Err(TimingFault::InterruptAcknowledgeUnsupported);
        }
        let reply = self.finish(response, access, Direction::Read)?;
        let vector = match reply {
            BusResponse::Complete(InterruptVector::Vector(vector)) => vector,
            BusResponse::Complete(InterruptVector::Autovector) => VECTOR_AUTOVECTOR_BASE + level,
            BusResponse::Complete(InterruptVector::Spurious) => VECTOR_SPURIOUS_INTERRUPT,
            BusResponse::Unsupported => unreachable!(),
        };
        if level == 7 {
            self.interrupts.level7_edge = false;
        }
        self.idle(bus, 4)?;
        self.write_word(bus, sp.wrapping_sub(6), sr)?;
        self.write_word(bus, sp.wrapping_sub(4), (pc >> 16) as u16)?;
        let vector_address = u32::from(vector) * VECTOR_ENTRY_BYTES;
        let high = self.read_word(
            bus,
            vector_address,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
        )?;
        let low = self.read_word(
            bus,
            vector_address + 2,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
        )?;
        let target = (u32::from(high) << 16) | u32::from(low);
        let first = self.read_word(
            bus,
            target,
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
        )?;
        self.idle(bus, 2)?;
        let second = self.read_word(
            bus,
            target.wrapping_add(2),
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
        )?;
        self.registers.ssp = sp.wrapping_sub(6);
        self.registers.set_status_register(
            ((sr | SUPERVISOR) & !(TRACE | INTERRUPT_MASK)) | (u16::from(level) << 8),
        );
        self.registers.pc = target;
        self.pipeline = Some(Pipeline {
            address: target,
            words: [first, second],
        });
        self.stopped = false;
        Ok(TimedOutcome::Interrupt {
            level,
            vector,
            clocks: self.clock.0 - start,
        })
    }
}

#[cfg(test)]
mod tests;

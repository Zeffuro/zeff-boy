//! Instruction-specific prefetch with full-transfer CPU clock boundaries.
//! Bus adapters own device advancement during transfers; pin states are not modeled.
//! Host faults freeze execution without retry; completed effects remain observable.
//! Selected physical captures use a separately supplied exception-entry boundary.

use crate::bus::{
    AccessKind, BUS_ADDRESS_MASK, BusAccess, BusResponse, ByteLanes, Direction, FunctionCode,
};
use crate::constants::{TRANSFER_CLOCKS, VECTOR_TRACE};
use crate::{Registers, TRACE};

mod binary;
mod bit;
mod branch_conditions;
mod control;
mod control_address;
mod frame;
mod immediate;
mod interrupt;
mod math;
mod moves;
mod operand;
mod owned_bus;
mod paired;
pub mod physical;
pub mod physical_instruction;
mod register;
mod shift;
mod state;
mod status;
mod transfer;
mod unary;
pub use interrupt::{InterruptState, InterruptVector, TimedOutcome};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CpuClocks(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CycleResult<T, E> {
    pub result: Result<T, E>,
    pub wait_clocks: u32,
}

impl<T, E> CycleResult<T, E> {
    pub fn ready(value: T) -> Self {
        Self {
            result: Ok(value),
            wait_clocks: 0,
        }
    }
}

/// Each transfer advances devices by four CPU clocks plus its returned waits.
/// Data/effects commit within that callback; idle advances devices without a transfer.
/// A callback error is a host failure, not a physical BERR signal.
pub trait ClockedBus {
    type Error;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, Self::Error>;
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error>;
    fn idle(&mut self, start: CpuClocks, clocks: u32);
    /// Complete acquires output until end; other outcomes return unowned. Neither edge advances time.
    fn begin_reset_output(&mut self, _start: CpuClocks) -> Result<BusResponse<()>, Self::Error> {
        Ok(BusResponse::Unsupported)
    }
    /// Implement alongside a supported begin hook; release is infallible and takes no time.
    fn end_reset_output(&mut self, _end: CpuClocks) {}
    /// Zero-time begin: Complete owns byte R/idle/W; other outcomes clean any acquisition.
    fn begin_rmw(
        &mut self,
        _access: BusAccess,
        _start: CpuClocks,
    ) -> Result<BusResponse<()>, Self::Error> {
        Ok(BusResponse::Unsupported)
    }
    /// Release ownership before any prefetch, without advancing devices.
    fn end_rmw(&mut self, _end: CpuClocks) {}
    fn acknowledge_cycle(
        &mut self,
        _access: BusAccess,
        _start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, Self::Error> {
        CycleResult::ready(BusResponse::Unsupported)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimingFault<E> {
    AddressError {
        address: u32,
    },
    Bus {
        access: BusAccess,
        direction: Direction,
        source: E,
    },
    UnsupportedInstruction {
        pc: u32,
        opcode: u16,
    },
    Unprimed,
    Frozen,
    ClockOverflow,
    InterruptAcknowledgeUnsupported,
    ResetOutputUnsupported,
    ResetOutput {
        source: E,
    },
    RmwOwnershipUnsupported {
        access: BusAccess,
    },
    RmwOwnership {
        access: BusAccess,
        source: E,
    },
    Stopped,
    PendingTrace,
    DoubleFaultHalted,
    ImportedBoundaryPending,
    PhysicalFaultUnqualified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedStep {
    pub opcode: u16,
    pub clocks: u64,
    /// Last exception entered; a TRAP followed by trace reports vector9.
    pub exception: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pipeline {
    address: u32,
    words: [u16; 2],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrefetchCpu {
    pub registers: Registers,
    clock: CpuClocks,
    pipeline: Option<Pipeline>,
    frozen: bool,
    stopped: bool,
    interrupts: InterruptState,
    pending_trace: bool,
    physical_state: physical::PhysicalEntryState,
    physical_inspection: physical::PhysicalInspection,
    imported_boundary: Option<physical::ImportedGroup0Boundary>,
}

impl PrefetchCpu {
    pub fn clock(&self) -> CpuClocks {
        self.clock
    }

    pub fn queued_words(&self) -> Option<[u16; 2]> {
        self.pipeline.map(|pipeline| pipeline.words)
    }

    /// Imports an instruction-boundary witness, including words already fetched.
    pub fn from_prefetched(
        registers: Registers,
        words: [u16; 2],
        clock: CpuClocks,
    ) -> Option<Self> {
        if registers.pc & 1 != 0 {
            return None;
        }
        Some(Self {
            pipeline: Some(Pipeline {
                address: registers.pc,
                words,
            }),
            registers,
            clock,
            frozen: false,
            stopped: false,
            interrupts: InterruptState::default(),
            pending_trace: false,
            physical_state: physical::PhysicalEntryState::Idle,
            physical_inspection: physical::PhysicalInspection::DiagnosticCompleteValues,
            imported_boundary: None,
        })
    }

    /// Primes two words explicitly; this is not the hardware reset exception.
    pub fn prime<B: ClockedBus>(&mut self, bus: &mut B) -> Result<(), TimingFault<B::Error>> {
        self.execution_guard()?;
        if self.stopped {
            return Err(TimingFault::Stopped);
        }
        let address = self.registers.pc;
        let first = self.fetch(bus, address)?;
        let second = self.fetch(bus, address.wrapping_add(2))?;
        self.pipeline = Some(Pipeline {
            address,
            words: [first, second],
        });
        Ok(())
    }

    /// Executes an instruction; `step_boundary` also samples the interrupt input.
    pub fn step<B: ClockedBus>(&mut self, bus: &mut B) -> Result<TimedStep, TimingFault<B::Error>> {
        let mut step = self.step_instruction(bus)?;
        if let Some(clocks) = self.service_pending_trace(bus)? {
            step.clocks += clocks;
            step.exception = Some(VECTOR_TRACE);
        }
        Ok(step)
    }

    /// Stops at the instruction boundary before a pending trace exception is serviced.
    pub fn step_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
    ) -> Result<TimedStep, TimingFault<B::Error>> {
        self.execution_guard()?;
        if self.pending_trace {
            return Err(TimingFault::PendingTrace);
        }
        if self.stopped {
            return Err(TimingFault::Stopped);
        }
        let trace = self.registers.status_register() & TRACE != 0;
        let pipeline = self
            .pipeline
            .filter(|pipe| pipe.address == self.registers.pc)
            .ok_or(TimingFault::Unprimed)?;
        let opcode = pipeline.words[0];
        self.physical_state = physical::PhysicalEntryState::Idle;
        let start = self.clock.0;
        let pc = self.registers.pc;
        let mut exception = None;
        let mut trace_allowed = true;
        if opcode == 0x4e71 || opcode & 0xf100 == 0x7000 {
            let next = self.fetch(bus, pc.wrapping_add(4))?;
            if opcode != 0x4e71 {
                let value = i32::from(opcode as u8 as i8) as u32;
                self.registers.d[usize::from((opcode >> 9) & 7)] = value;
                crate::alu::logic(&mut self.registers, crate::ea::Size::Long, value);
            }
            self.registers.pc = pc.wrapping_add(2);
            self.pipeline = Some(Pipeline {
                address: self.registers.pc,
                words: [pipeline.words[1], next],
            });
        } else if opcode & 0xf000 == 0x6000 && opcode & 0x0f00 != 0x0100 {
            let displacement = if opcode as u8 == 0 {
                i32::from(pipeline.words[1] as i16)
            } else {
                i32::from(opcode as u8 as i8)
            };
            if crate::alu::condition(self.registers.status_register(), (opcode >> 8) & 15) {
                let target = pc.wrapping_add(2).wrapping_add_signed(displacement);
                self.idle(bus, 2)?;
                let first = self.fetch(bus, target)?;
                let second = self.fetch(bus, target.wrapping_add(2))?;
                self.registers.pc = target;
                self.pipeline = Some(Pipeline {
                    address: target,
                    words: [first, second],
                });
            } else {
                self.idle(bus, 4)?;
                let first = self.fetch(bus, pc.wrapping_add(4))?;
                let (next_pc, words) = if opcode as u8 == 0 {
                    (
                        pc.wrapping_add(4),
                        [first, self.fetch(bus, pc.wrapping_add(6))?],
                    )
                } else {
                    (pc.wrapping_add(2), [pipeline.words[1], first])
                };
                self.registers.pc = next_pc;
                self.pipeline = Some(Pipeline {
                    address: next_pc,
                    words,
                });
            }
        } else if let Some(completion) = self.control(bus, pipeline)? {
            exception = completion.exception;
            trace_allowed = completion.trace;
        } else if let Some(completion) = self.frame_instruction(bus, pipeline)? {
            exception = completion.exception;
            trace_allowed = completion.trace;
        } else if let Some(completion) = self.status_instruction(bus, pipeline)? {
            exception = completion.exception;
            trace_allowed = completion.trace;
        } else if let Some(completion) = self.math_instruction(bus, pipeline)? {
            exception = completion.exception;
            trace_allowed = completion.trace;
        } else if self.move_instruction(bus, pipeline)?
            || self.branch_condition_instruction(bus, pipeline)?
            || self.control_address_instruction(bus, pipeline)?
            || self.transfer_instruction(bus, pipeline)?
            || self.tas_instruction(bus, pipeline)?
            || self.unary_instruction(bus, pipeline)?
            || self.binary_instruction(bus, pipeline)?
            || self.immediate_instruction(bus, pipeline)?
            || self.bit_instruction(bus, pipeline)?
            || self.memory_shift_instruction(bus, pipeline)?
            || self.paired_instruction(bus, pipeline)?
            || self.register_instruction(bus, pipeline)?
        {
        } else {
            return Err(TimingFault::UnsupportedInstruction { pc, opcode });
        }
        self.pending_trace = trace && trace_allowed;
        Ok(TimedStep {
            opcode,
            clocks: self.clock.0 - start,
            exception,
        })
    }

    pub fn trace_pending(&self) -> bool {
        self.pending_trace
    }

    pub fn service_pending_trace<B: ClockedBus>(
        &mut self,
        bus: &mut B,
    ) -> Result<Option<u64>, TimingFault<B::Error>> {
        self.execution_guard()?;
        if !self.pending_trace {
            return Ok(None);
        }
        let start = self.clock.0;
        self.short_exception(bus, VECTOR_TRACE, self.registers.pc)?;
        self.pending_trace = false;
        Ok(Some(self.clock.0 - start))
    }

    fn idle<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        clocks: u32,
    ) -> Result<(), TimingFault<B::Error>> {
        let Some(next) = self.clock.0.checked_add(u64::from(clocks)) else {
            self.frozen = true;
            return Err(TimingFault::ClockOverflow);
        };
        bus.idle(self.clock, clocks);
        self.clock = CpuClocks(next);
        Ok(())
    }

    fn fetch<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        address: u32,
    ) -> Result<u16, TimingFault<B::Error>> {
        self.read_word(
            bus,
            address,
            crate::ea::program_space(&self.registers),
            AccessKind::Prefetch,
        )
    }

    fn read_word<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        address: u32,
        function_code: FunctionCode,
        kind: AccessKind,
    ) -> Result<u16, TimingFault<B::Error>> {
        if address & 1 != 0 {
            self.frozen = true;
            return Err(TimingFault::AddressError { address });
        }
        let access = BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK,
            function_code,
            kind,
            lanes: ByteLanes::Both,
        };
        self.read_access(bus, access)
    }

    fn read_access<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        access: BusAccess,
    ) -> Result<u16, TimingFault<B::Error>> {
        self.reserve_transfer()?;
        let response = bus.read_cycle(access, self.clock);
        self.finish(response, access, Direction::Read)
    }

    fn write_word<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        address: u32,
        value: u16,
    ) -> Result<(), TimingFault<B::Error>> {
        if address & 1 != 0 {
            self.frozen = true;
            return Err(TimingFault::AddressError { address });
        }
        let access = BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK,
            function_code: FunctionCode::SupervisorData,
            kind: AccessKind::Stack,
            lanes: ByteLanes::Both,
        };
        self.write_access(bus, access, value)
    }

    fn write_access<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        access: BusAccess,
        value: u16,
    ) -> Result<(), TimingFault<B::Error>> {
        self.reserve_transfer()?;
        let response = bus.write_cycle(access, value, self.clock);
        self.finish(response, access, Direction::Write)
    }

    fn reserve_transfer<E>(&mut self) -> Result<(), TimingFault<E>> {
        if self
            .clock
            .0
            .checked_add(u64::from(TRANSFER_CLOCKS) + u64::from(u32::MAX))
            .is_none()
        {
            self.frozen = true;
            return Err(TimingFault::ClockOverflow);
        }
        Ok(())
    }

    fn finish<T, E>(
        &mut self,
        response: CycleResult<T, E>,
        access: BusAccess,
        direction: Direction,
    ) -> Result<T, TimingFault<E>> {
        self.clock.0 += u64::from(TRANSFER_CLOCKS) + u64::from(response.wait_clocks);
        response.result.map_err(|source| {
            self.frozen = true;
            TimingFault::Bus {
                access,
                direction,
                source,
            }
        })
    }
}

#[cfg(test)]
mod test_bus;
#[cfg(test)]
mod tests;

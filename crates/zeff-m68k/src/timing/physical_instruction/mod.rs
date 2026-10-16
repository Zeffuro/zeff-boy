use super::physical::{CriticalFault, EntryBus, EntryFault, Group0Capture, PhysicalEntryState};
use super::{CpuClocks, Pipeline, PrefetchCpu, TimedStep, TimingFault};
use crate::bus::{BusAccess, Direction};
use crate::constants::TRACE;

mod address_arithmetic;
mod classifier;
mod cmp;
mod cmpi;
mod data_arithmetic;
mod extension_moves;
mod from_sr;
mod immediate_moves;
mod memory_moves;
mod memory_reads;
mod moves;
mod register_moves;
mod registers;
mod rmw;
mod status;
mod stores;
mod transfer;
mod tst;
use registers::MoveRegister;
use transfer::{TransferExit, TransferPhase};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstructionOutcome<T, E> {
    Complete(T),
    HostFailure(E),
    AtGroup0Entry,
    TimingUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstructionReport<T, E> {
    pub elapsed: u32,
    pub outcome: InstructionOutcome<T, E>,
}

/// Total elapsed belongs to the device callback. AtGroup0Entry includes abort,
/// release and transition; the CPU adds only the subsequent entry schedule.
/// Host failures retain completed phase effects, including Dn/CCR changes and writes.
pub trait InstructionBus: EntryBus {
    /// Operands and prefetch share physical D15..D0 lane placement.
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, Self::Error>;

    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), Self::Error>;

    /// Detection only; AtGroup0Entry ends at aser1. Complete supplies no such boundary.
    fn detect_instruction_address_error(
        &mut self,
        _access: BusAccess,
        _direction: Direction,
        _start: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        InstructionReport {
            elapsed: 0,
            outcome: InstructionOutcome::TimingUnavailable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstructionPhase {
    SequentialRefill,
    BranchFirst,
    BranchSecond,
    ImmediateFirst,
    ImmediateSecond,
    ExtensionFirst,
    ExtensionSecond,
    OperandReadHigh,
    OperandReadLow,
    OperandWriteHigh,
    OperandWriteLow,
    OperandRefill,
    StatusFirst,
    StatusSecond,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicalInstructionStep {
    Instruction(TimedStep),
    Group0 {
        opcode: u16,
        capture: Group0Capture,
        clocks: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstructionFault<E> {
    Execution(TimingFault<E>),
    Entry(EntryFault<E>),
    TraceUnqualified,
    InterruptUnqualified,
    CaptureUnavailable {
        phase: InstructionPhase,
        cause: CriticalFault,
    },
    TransitionTimingUnavailable {
        phase: InstructionPhase,
        cause: CriticalFault,
    },
}

fn selected_instruction(word: u16, supervisor: bool) -> bool {
    word == 0x4e71
        || word & 0xf100 == 0x7000
        || word & 0xff00 == 0x6000
        || moves::MoveProfile::decode(word).is_some()
        || extension_moves::ExtensionMoveProfile::decode(word).is_some()
        || register_moves::RegisterMoveProfile::decode(word).is_some()
        || immediate_moves::ImmediateMoveProfile::decode(word).is_some()
        || memory_moves::MemoryMoveProfile::decode(word).is_some()
        || tst::TstProfile::decode(word).is_some()
        || rmw::RmwProfile::decode(word).is_some()
        || cmp::CmpProfile::decode(word).is_some()
        || cmpi::CmpiProfile::decode(word).is_some()
        || status::StatusProfile::decode(word, supervisor).is_some()
        || from_sr::FromSrProfile::decode(word).is_some()
        || address_arithmetic::AddressArithmeticProfile::decode(word).is_some()
        || data_arithmetic::DataArithmeticProfile::decode(word).is_some()
}

impl PrefetchCpu {
    /// Selected instruction captures with supplied transition timing.
    pub fn step_physical_instruction<B: InstructionBus>(
        &mut self,
        bus: &mut B,
    ) -> Result<PhysicalInstructionStep, InstructionFault<B::Error>> {
        self.execution_guard()
            .map_err(InstructionFault::Execution)?;
        if self.pending_trace {
            return Err(InstructionFault::Execution(TimingFault::PendingTrace));
        }
        if self.stopped {
            return Err(InstructionFault::Execution(TimingFault::Stopped));
        }
        if self.registers.status_register() & TRACE != 0 {
            return Err(InstructionFault::TraceUnqualified);
        }
        if self
            .interrupts
            .accepted(self.registers.status_register())
            .is_some()
        {
            return Err(InstructionFault::InterruptUnqualified);
        }
        let pipeline = self
            .pipeline
            .filter(|p| p.address == self.registers.pc)
            .ok_or(InstructionFault::Execution(TimingFault::Unprimed))?;
        let opcode = pipeline.words[0];
        if !selected_instruction(opcode, self.registers.supervisor()) {
            return Err(InstructionFault::Execution(
                TimingFault::UnsupportedInstruction {
                    pc: self.registers.pc,
                    opcode,
                },
            ));
        }
        self.physical_state = PhysicalEntryState::Idle;
        let start = self.clock.0;
        let pc = self.registers.pc;
        if let Some(profile) = register_moves::RegisterMoveProfile::decode(opcode) {
            if let Err(exit) = self.physical_register_move(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = extension_moves::ExtensionMoveProfile::decode(opcode) {
            if let Err(exit) = self.physical_extension_move(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = moves::MoveProfile::decode(opcode) {
            if let Err(exit) = self.physical_move(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = immediate_moves::ImmediateMoveProfile::decode(opcode) {
            if let Err(exit) = self.physical_immediate_move(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = memory_moves::MemoryMoveProfile::decode(opcode) {
            if let Err(exit) = self.physical_memory_move(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = tst::TstProfile::decode(opcode) {
            if let Err(exit) = self.physical_tst(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = rmw::RmwProfile::decode(opcode) {
            if let Err(exit) = self.physical_rmw(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = cmp::CmpProfile::decode(opcode) {
            if let Err(exit) = self.physical_cmp(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = cmpi::CmpiProfile::decode(opcode) {
            if let Err(exit) = self.physical_cmpi(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = from_sr::FromSrProfile::decode(opcode) {
            if let Err(exit) = self.physical_from_sr(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) =
            status::StatusProfile::decode(opcode, self.registers.supervisor())
        {
            if let Err(exit) = self.physical_status(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = address_arithmetic::AddressArithmeticProfile::decode(opcode) {
            if let Err(exit) = self.physical_address_arithmetic(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if let Some(profile) = data_arithmetic::DataArithmeticProfile::decode(opcode) {
            if let Err(exit) = self.physical_data_arithmetic(bus, pipeline, profile) {
                return Self::physical_exit(exit, opcode, start, self.clock.0);
            }
        } else if opcode & 0xff00 == 0x6000 {
            let displacement = if opcode as u8 == 0 {
                i32::from(pipeline.words[1] as i16)
            } else {
                i32::from(opcode as u8 as i8)
            };
            let target = pc.wrapping_add(2).wrapping_add_signed(displacement);
            self.entry_idle(bus, 2).map_err(InstructionFault::Entry)?;
            let phase = self.prefetch_phase(
                InstructionPhase::BranchFirst,
                target,
                pc.wrapping_add(2),
                opcode,
            );
            let first = match self.physical_read(bus, phase) {
                Ok(value) => value,
                Err(exit) => return Self::physical_exit(exit, opcode, start, self.clock.0),
            };
            let phase = self.prefetch_phase(
                InstructionPhase::BranchSecond,
                target.wrapping_add(2),
                target.wrapping_add(2),
                first,
            );
            let second = match self.physical_read(bus, phase) {
                Ok(value) => value,
                Err(exit) => return Self::physical_exit(exit, opcode, start, self.clock.0),
            };
            self.registers.pc = target;
            self.pipeline = Some(Pipeline {
                address: target,
                words: [first, second],
            });
        } else {
            if opcode != 0x4e71 {
                self.entry_reserve().map_err(InstructionFault::Entry)?;
                let value = i32::from(opcode as u8 as i8) as u32;
                self.registers.d[usize::from((opcode >> 9) & 7)] = value;
                crate::alu::logic(&mut self.registers, crate::ea::Size::Long, value);
            }
            let phase = self.prefetch_phase(
                InstructionPhase::SequentialRefill,
                pc.wrapping_add(4),
                pc.wrapping_add(4),
                pipeline.words[1],
            );
            let next = match self.physical_read(bus, phase) {
                Ok(value) => value,
                Err(exit) => return Self::physical_exit(exit, opcode, start, self.clock.0),
            };
            self.registers.pc = pc.wrapping_add(2);
            self.pipeline = Some(Pipeline {
                address: self.registers.pc,
                words: [pipeline.words[1], next],
            });
        }
        Ok(PhysicalInstructionStep::Instruction(TimedStep {
            opcode,
            clocks: self.clock.0 - start,
            exception: None,
        }))
    }

    fn physical_exit<E>(
        exit: TransferExit<E>,
        opcode: u16,
        start: u64,
        end: u64,
    ) -> Result<PhysicalInstructionStep, InstructionFault<E>> {
        match exit {
            TransferExit::Entered(capture) => Ok(PhysicalInstructionStep::Group0 {
                opcode,
                capture,
                clocks: end - start,
            }),
            TransferExit::Fault(fault) => Err(fault),
        }
    }
}

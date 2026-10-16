//! M68000 architectural execution with demand fetch and nominal clocks.
//! `Cpu` omits prefetch, waits, trace, interrupts and group-0 fault processing.
//! Faults restore CPU state; completed bus effects remain observable.
//! `timing` provides persistent prefetch and physical transfer clocks.

#![forbid(unsafe_code)]

use crate::constants::{RESET_PC_VECTOR_ADDRESS, RESET_SSP_VECTOR_ADDRESS};
use std::num::NonZeroU64;

use zeff_emu_common::cpu::{CpuCore, CpuStep};

pub mod bus;
mod constants;
pub mod disassembly;
pub mod ea;
mod instructions;
mod registers;
pub mod timing;

pub(crate) use instructions::{
    alu, bcd, bit, bus_instruction, control, data, exception, extend, instruction, multiply, shift,
    transfer,
};

pub use constants::{CARRY, EXTEND, NEGATIVE, OVERFLOW, STATUS_MASK, SUPERVISOR, TRACE, ZERO};
pub use registers::Registers;

use bus::{AccessKind, BusFault, FunctionCode, M68kBus, read_long, read_word};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault<E> {
    Bus(BusFault<E>),
    UnsupportedInstruction { pc: u32, opcode: u16 },
    TraceUnsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome<E> {
    Executed {
        opcode: u16,
        nominal_cycles: u32,
    },
    Exception {
        opcode: u16,
        vector: u8,
        nominal_cycles: u32,
    },
    Stopped,
    Fault(Fault<E>),
}

impl<E> CpuStep for StepOutcome<E> {
    fn cpu_cycles(&self) -> Option<NonZeroU64> {
        match self {
            Self::Executed { nominal_cycles, .. } | Self::Exception { nominal_cycles, .. } => {
                NonZeroU64::new(u64::from(*nominal_cycles))
            }
            Self::Fault(_) | Self::Stopped => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cpu {
    pub registers: Registers,
    pub stopped: bool,
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Undefined general-register/CCR reset values use deterministic zero policy.
    /// CPU state commits after all vector reads; bus side effects are not rolled back.
    pub fn reset<B: M68kBus>(&mut self, bus: &mut B) -> Result<(), Fault<B::Error>> {
        let ssp = read_long(
            bus,
            RESET_SSP_VECTOR_ADDRESS,
            FunctionCode::SupervisorProgram,
            AccessKind::ResetVector,
        )
        .map_err(Fault::Bus)?;
        let pc = read_long(
            bus,
            RESET_PC_VECTOR_ADDRESS,
            FunctionCode::SupervisorProgram,
            AccessKind::ResetVector,
        )
        .map_err(Fault::Bus)?;
        let mut registers = Registers::new();
        registers.ssp = ssp;
        registers.pc = pc;
        self.registers = registers;
        self.stopped = false;
        Ok(())
    }

    pub fn step<B: M68kBus>(&mut self, bus: &mut B) -> StepOutcome<B::Error> {
        if self.stopped {
            return StepOutcome::Stopped;
        }
        let original = self.registers;
        match self.execute(bus) {
            Ok((opcode, execution)) => {
                self.stopped = execution.stopped;
                let nominal_cycles = execution.nominal_cycles;
                match execution.exception {
                    Some(vector) => StepOutcome::Exception {
                        opcode,
                        vector,
                        nominal_cycles,
                    },
                    None => StepOutcome::Executed {
                        opcode,
                        nominal_cycles,
                    },
                }
            }
            Err(fault) => {
                self.registers = original;
                StepOutcome::Fault(fault)
            }
        }
    }

    fn execute<B: M68kBus>(
        &mut self,
        bus: &mut B,
    ) -> Result<(u16, instruction::Execution), Fault<B::Error>> {
        if self.registers.status_register() & TRACE != 0 {
            return Err(Fault::TraceUnsupported);
        }
        let pc = self.registers.pc;
        let program = if self.registers.supervisor() {
            FunctionCode::SupervisorProgram
        } else {
            FunctionCode::UserProgram
        };
        let opcode = read_word(bus, pc, program, AccessKind::Opcode).map_err(Fault::Bus)?;
        let execution = instruction::execute(&mut self.registers, bus, pc, opcode)?;
        Ok((opcode, execution))
    }
}

impl<B: M68kBus> CpuCore<B> for Cpu {
    type Step = StepOutcome<B::Error>;

    fn step_cpu(&mut self, bus: &mut B) -> Self::Step {
        self.step(bus)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
use tests::{conformance_tests, data_tests, shift_tests, test_bus};

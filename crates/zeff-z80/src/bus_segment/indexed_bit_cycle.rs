use super::*;
use indexed_memory_cycle::FrozenReceipt;
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexedCbCycleKind {
    Displacement {
        address: u16,
        preceding_refresh: u16,
    },
    Terminal {
        address: u16,
    },
    Read {
        address: u16,
    },
    ReadModify {
        address: u16,
    },
    Write {
        address: u16,
        value: u8,
    },
}

impl IndexedCbCycleKind {
    pub fn address(self) -> u16 {
        match self {
            Self::Displacement { address, .. }
            | Self::Terminal { address }
            | Self::Read { address }
            | Self::ReadModify { address }
            | Self::Write { address, .. } => address,
        }
    }
    pub fn t_states(self) -> u32 {
        match self {
            Self::Displacement { .. } | Self::Write { .. } => 3,
            Self::Terminal { .. } => 5,
            Self::Read { .. } | Self::ReadModify { .. } => 4,
        }
    }
    pub fn finishes(self) -> bool {
        matches!(self, Self::Read { .. } | Self::Write { .. })
    }
}

#[derive(Debug)]
pub struct IndexedCbCycleRequest {
    cpu: Cpu,
    bus: BusState,
    kind: IndexedCbCycleKind,
}

#[derive(Debug)]
pub struct IndexedCbCycleCompletion {
    request: IndexedCbCycleRequest,
    receipt: IndexedMemoryReceipt,
    waits: u32,
}

impl Cpu {
    pub fn indexed_cb_cycle_pending(&self, bus: &BusState) -> bool {
        matches!(
            bus.transfer,
            Some(Transfer {
                operation: Operation::IndexedCb(_),
                ..
            })
        )
    }
}

impl IndexedCbCycleRequest {
    pub fn kind(&self) -> IndexedCbCycleKind {
        self.kind
    }
    pub fn preceding_address(&self) -> u16 {
        match self.kind {
            IndexedCbCycleKind::Displacement {
                preceding_refresh, ..
            } => preceding_refresh,
            IndexedCbCycleKind::Write { address, .. } => address,
            _ => self.cpu.regs.pc.wrapping_sub(1),
        }
    }
    pub fn instruction(&self) -> FetchedInstruction {
        FetchedInstruction {
            pc: self.cpu.last_opcode_pc,
            opcode: self.cpu.last_opcode,
            cycles: self.kind.t_states(),
        }
    }
    pub fn complete(
        self,
        receipt: IndexedMemoryReceipt,
        waits: u32,
    ) -> Result<IndexedCbCycleCompletion, CycleError> {
        match (self.kind, receipt) {
            (
                IndexedCbCycleKind::Write { address, value },
                IndexedMemoryReceipt::Write {
                    address: actual,
                    value: sampled,
                },
            ) if address == actual && value == sampled => {}
            (IndexedCbCycleKind::Write { .. }, _) => return Err(CycleError::SampleMismatch),
            (
                _,
                IndexedMemoryReceipt::Read {
                    address,
                    known_mask,
                    ..
                },
            ) => {
                if address != self.kind.address() {
                    return Err(CycleError::SampleMismatch);
                }
                if known_mask != 255 {
                    return Err(CycleError::UnknownData);
                }
            }
            _ => return Err(CycleError::SampleMismatch),
        }
        let span = self
            .kind
            .t_states()
            .checked_add(waits)
            .ok_or(CycleError::ClockOverflow)?;
        self.cpu
            .cycles
            .checked_add(u64::from(span))
            .ok_or(CycleError::ClockOverflow)?;
        Ok(IndexedCbCycleCompletion {
            request: self,
            receipt,
            waits,
        })
    }
}

impl Cpu {
    pub fn next_indexed_cb_cycle(
        &self,
        bus: &BusState,
    ) -> Result<IndexedCbCycleRequest, CycleError> {
        if self.state != CpuState::Running
            || self.trap.is_some()
            || self.last_step_was_interrupt
            || self.enable_interrupts_delay > 2
        {
            return Err(CycleError::UnsupportedState);
        }
        let Some(Transfer {
            pc,
            operation: Operation::IndexedCb(operation),
        }) = bus.transfer
        else {
            return Err(CycleError::UnsupportedCycle);
        };
        if !(2..=4).contains(&self.instruction_byte_count) {
            return Err(CycleError::UnsupportedCycle);
        }
        let (first, prefix) = self
            .index_context(bus, self.instruction_byte_count, 2)
            .ok_or(CycleError::UnsupportedCycle)?;
        if pc != first
            || self.instruction_bytes[1] != Z80_PREFIX_CB
            || self.instruction_bytes[usize::from(self.instruction_byte_count)..]
                .iter()
                .any(|b| *b != 0)
        {
            return Err(CycleError::UnsupportedCycle);
        }
        let kind = operation.typed_kind(self, prefix)?;
        Ok(IndexedCbCycleRequest {
            cpu: self.clone(),
            bus: *bus,
            kind,
        })
    }

    pub fn retire_indexed_cb_cycle(
        &mut self,
        bus: &mut BusState,
        completion: IndexedCbCycleCompletion,
    ) -> Result<BoundedStep, CycleError> {
        self.retire_indexed_cb_cycle_capturing(bus, completion, &retirement_evidence::NoCapture)
    }

    pub fn retire_indexed_cb_cycle_with_evidence(
        &mut self,
        bus: &mut BusState,
        completion: IndexedCbCycleCompletion,
    ) -> Result<(BoundedStep, RetirementEvidence), CycleError> {
        let before = self.cycles;
        let capture = retirement_evidence::InputCapture::default();
        let step = self.retire_indexed_cb_cycle_capturing(bus, completion, &capture)?;
        let mut evidence = RetirementEvidence::new(
            step.complete,
            capture.get(),
            before,
            self.cycles,
            RetirementKind::Instruction,
        );
        evidence.input_overflowed = capture.overflowed();
        Ok((step, evidence))
    }

    fn retire_indexed_cb_cycle_capturing<C: retirement_evidence::Capture>(
        &mut self,
        bus: &mut BusState,
        completion: IndexedCbCycleCompletion,
        capture: &C,
    ) -> Result<BoundedStep, CycleError> {
        let IndexedCbCycleCompletion {
            request,
            receipt,
            waits,
        } = completion;
        if request.cpu != *self || request.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        if self.next_indexed_cb_cycle(bus)?.kind != request.kind {
            return Err(CycleError::InvalidCompletion);
        }
        let span = request
            .kind
            .t_states()
            .checked_add(waits)
            .ok_or(CycleError::ClockOverflow)?;
        let total = self
            .cycles
            .checked_add(u64::from(span))
            .ok_or(CycleError::ClockOverflow)?;
        let mut frozen = FrozenReceipt {
            capture,
            class: match request.kind {
                IndexedCbCycleKind::Displacement { .. } => InstructionByteClass::Displacement,
                _ => InstructionByteClass::Terminal,
            },
            receipt,
            consumed: Cell::new(false),
            invalid: Cell::new(false),
        };
        let mut next = self.clone();
        let mut after = *bus;
        let mut result = next
            .step_bus_segment(&mut frozen, &mut after)
            .ok_or(CycleError::InvalidCompletion)?;
        if frozen.invalid.get()
            || !frozen.consumed.get()
            || result.complete != request.kind.finishes()
            || result.complete && after.pending()
            || result.instruction != request.instruction()
            || next.last_step_was_interrupt
            || next.trap.is_some()
            || !next.extend_bus_segment(&mut result.instruction, waits)
            || next.cycles != total
        {
            return Err(CycleError::InvalidCompletion);
        }
        if next.indexed_cb_cycle_pending(&after) {
            next.next_indexed_cb_cycle(&after)?;
        }
        *self = next;
        *bus = after;
        Ok(result)
    }
}

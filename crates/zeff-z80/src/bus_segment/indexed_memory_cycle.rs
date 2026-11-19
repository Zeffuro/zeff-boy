use super::*;
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexedMemoryKind {
    Displacement {
        address: u16,
        preceding_refresh: u16,
    },
    Address {
        retained_address: u16,
    },
    Immediate {
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

impl IndexedMemoryKind {
    pub fn address(self) -> u16 {
        match self {
            Self::Displacement { address, .. }
            | Self::Immediate { address }
            | Self::Read { address }
            | Self::ReadModify { address }
            | Self::Write { address, .. } => address,
            Self::Address { retained_address } => retained_address,
        }
    }
    pub fn t_states(self) -> u32 {
        match self {
            Self::Address { .. } | Self::Immediate { .. } => 5,
            Self::ReadModify { .. } => 4,
            _ => 3,
        }
    }
    pub fn finishes(self) -> bool {
        matches!(self, Self::Read { .. } | Self::Write { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexedMemoryReceipt {
    Read {
        address: u16,
        value: u8,
        known_mask: u8,
    },
    Write {
        address: u16,
        value: u8,
    },
    Internal,
}

#[derive(Debug)]
pub struct IndexedMemoryRequest {
    cpu: Cpu,
    bus: BusState,
    kind: IndexedMemoryKind,
}

#[derive(Debug)]
pub struct IndexedMemoryCompletion {
    request: IndexedMemoryRequest,
    receipt: IndexedMemoryReceipt,
    waits: u32,
}

impl BusState {
    pub fn indexed_memory_pending(&self) -> bool {
        matches!(self.transfer, Some(Transfer { operation: Operation::IndexedMemory(op), .. }) if op.typed_pending())
    }
}

impl IndexedMemoryRequest {
    pub fn kind(&self) -> IndexedMemoryKind {
        self.kind
    }
    pub fn preceding_address(&self) -> u16 {
        match self.kind {
            IndexedMemoryKind::Displacement {
                preceding_refresh, ..
            } => preceding_refresh,
            IndexedMemoryKind::Write { address, .. }
                if matches!(self.cpu.instruction_bytes[1], 0x34 | 0x35) =>
            {
                address
            }
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
    ) -> Result<IndexedMemoryCompletion, CycleError> {
        match (self.kind, receipt) {
            (
                IndexedMemoryKind::Displacement { address, .. }
                | IndexedMemoryKind::Immediate { address }
                | IndexedMemoryKind::Read { address }
                | IndexedMemoryKind::ReadModify { address },
                IndexedMemoryReceipt::Read {
                    address: a,
                    known_mask,
                    ..
                },
            ) => {
                if address != a {
                    return Err(CycleError::SampleMismatch);
                }
                if known_mask != 255 {
                    return Err(CycleError::UnknownData);
                }
            }
            (
                IndexedMemoryKind::Write { address, value },
                IndexedMemoryReceipt::Write {
                    address: a,
                    value: v,
                },
            ) if address == a && value == v => {}
            (IndexedMemoryKind::Address { .. }, IndexedMemoryReceipt::Internal) if waits == 0 => {}
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
        Ok(IndexedMemoryCompletion {
            request: self,
            receipt,
            waits,
        })
    }
}

impl Cpu {
    pub fn next_indexed_memory_cycle(
        &self,
        bus: &BusState,
    ) -> Result<IndexedMemoryRequest, CycleError> {
        if self.state != CpuState::Running
            || self.trap.is_some()
            || self.last_step_was_interrupt
            || self.enable_interrupts_delay > 2
        {
            return Err(CycleError::UnsupportedState);
        }
        let Some(Transfer {
            pc,
            operation: Operation::IndexedMemory(operation),
        }) = bus.transfer
        else {
            return Err(CycleError::UnsupportedCycle);
        };
        let (first, prefix) = self
            .index_context(bus, self.instruction_byte_count, 2)
            .ok_or(CycleError::UnsupportedCycle)?;
        if pc != first
            || self.instruction_bytes[usize::from(self.instruction_byte_count)..]
                .iter()
                .any(|b| *b != 0)
        {
            return Err(CycleError::UnsupportedCycle);
        }
        let kind = operation.typed_kind(self, self.instruction_bytes[1], prefix)?;
        Ok(IndexedMemoryRequest {
            cpu: self.clone(),
            bus: *bus,
            kind,
        })
    }

    pub fn retire_indexed_memory_cycle(
        &mut self,
        bus: &mut BusState,
        completion: IndexedMemoryCompletion,
    ) -> Result<BoundedStep, CycleError> {
        self.retire_indexed_memory_cycle_capturing(bus, completion, &retirement_evidence::NoCapture)
    }

    pub fn retire_indexed_memory_cycle_with_evidence(
        &mut self,
        bus: &mut BusState,
        completion: IndexedMemoryCompletion,
    ) -> Result<(BoundedStep, RetirementEvidence), CycleError> {
        let before = self.cycles;
        let capture = retirement_evidence::InputCapture::default();
        let step = self.retire_indexed_memory_cycle_capturing(bus, completion, &capture)?;
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

    fn retire_indexed_memory_cycle_capturing<C: retirement_evidence::Capture>(
        &mut self,
        bus: &mut BusState,
        completion: IndexedMemoryCompletion,
        capture: &C,
    ) -> Result<BoundedStep, CycleError> {
        let IndexedMemoryCompletion {
            request,
            receipt,
            waits,
        } = completion;
        if request.cpu != *self || request.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        if self.next_indexed_memory_cycle(bus)?.kind != request.kind {
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
                IndexedMemoryKind::Displacement { .. } => InstructionByteClass::Displacement,
                _ => InstructionByteClass::Operand,
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
            || frozen.consumed.get() != !matches!(receipt, IndexedMemoryReceipt::Internal)
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
        if !result.complete {
            next.next_indexed_memory_cycle(&after)?;
        }
        *self = next;
        *bus = after;
        Ok(result)
    }
}

pub(super) struct FrozenReceipt<'a, C> {
    pub(super) capture: &'a C,
    pub(super) class: InstructionByteClass,
    pub(super) receipt: IndexedMemoryReceipt,
    pub(super) consumed: Cell<bool>,
    pub(super) invalid: Cell<bool>,
}
impl<C> FrozenReceipt<'_, C> {
    fn consume(&self) {
        self.invalid
            .set(self.invalid.get() || self.consumed.replace(true));
    }
}
impl<C: retirement_evidence::Capture> Z80Bus for FrozenReceipt<'_, C> {
    fn cpu_read_instruction(&self, address: u16) -> u8 {
        let value = self.cpu_read(address);
        self.capture.record(ConsumedInstructionByte {
            address,
            value,
            class: self.class,
        });
        value
    }
    fn cpu_read(&self, address: u16) -> u8 {
        self.consume();
        match self.receipt {
            IndexedMemoryReceipt::Read {
                address: a,
                value,
                known_mask: 255,
            } if address == a => value,
            _ => {
                self.invalid.set(true);
                0
            }
        }
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        self.consume();
        self.invalid.set(
            self.invalid.get() || self.receipt != IndexedMemoryReceipt::Write { address, value },
        );
    }
    fn io_read(&mut self, _: u8) -> u8 {
        self.invalid.set(true);
        0
    }
    fn io_write(&mut self, _: u8, _: u8) {
        self.invalid.set(true);
    }
    fn maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.invalid.set(true);
        false
    }
}

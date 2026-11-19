use super::*;
use std::cell::Cell;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordAddPhase {
    Internal4,
    Internal3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordAddKind {
    M1 {
        address: u16,
        refresh_address: u16,
    },
    Internal {
        retained_address: u16,
        phase: WordAddPhase,
    },
}

impl WordAddKind {
    pub fn address(self) -> u16 {
        match self {
            Self::M1 { address, .. } => address,
            Self::Internal {
                retained_address, ..
            } => retained_address,
        }
    }
    pub fn t_states(self) -> u32 {
        match self {
            Self::Internal {
                phase: WordAddPhase::Internal3,
                ..
            } => 3,
            _ => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordAddReceipt {
    M1 {
        address: u16,
        opcode: u8,
        known_mask: u8,
    },
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordAddTail {
    cpu: Cpu,
    bus: BusState,
    opcode: u8,
    waits: u32,
    phase: WordAddPhase,
}

impl WordAddTail {
    pub fn phase(&self) -> WordAddPhase {
        self.phase
    }
    pub fn opcode(&self) -> u8 {
        self.opcode
    }
    pub fn m1_waits(&self) -> u32 {
        self.waits
    }
    pub fn retained_address(&self) -> u16 {
        u16::from_be_bytes([self.cpu.regs.i, self.cpu.regs.r])
    }
    pub fn validate(&self, cpu: &Cpu, bus: &BusState) -> Result<(), CycleError> {
        if self.cpu != *cpu || self.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        cpu.word_add_m1(bus)?;
        if !word_opcode(self.opcode) {
            return Err(CycleError::UnsupportedOpcode {
                pc: cpu.regs.pc,
                opcode: self.opcode,
            });
        }
        let span = 11_u32
            .checked_add(self.waits)
            .ok_or(CycleError::ClockOverflow)?;
        cpu.cycles
            .checked_add(u64::from(span))
            .ok_or(CycleError::ClockOverflow)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct WordAddRequest {
    cpu: Cpu,
    bus: BusState,
    kind: WordAddKind,
    tail: Option<WordAddTail>,
}

#[derive(Debug)]
pub struct WordAddCompletion {
    request: WordAddRequest,
    receipt: WordAddReceipt,
    waits: u32,
}

#[derive(Debug)]
pub struct WordAddRetirement {
    pub step: BoundedStep,
    pub accounted_t_states: u32,
    pub tail: Option<WordAddTail>,
}

fn word_opcode(opcode: u8) -> bool {
    matches!(opcode, 0x09 | 0x19 | 0x29 | 0x39)
}

impl WordAddRequest {
    pub fn kind(&self) -> WordAddKind {
        self.kind
    }
    pub fn instruction(&self, opcode: u8) -> Result<FetchedInstruction, CycleError> {
        if !word_opcode(opcode) || self.tail.as_ref().is_some_and(|tail| tail.opcode != opcode) {
            return Err(CycleError::UnsupportedOpcode {
                pc: self.cpu.regs.pc,
                opcode,
            });
        }
        let (pc, opcode) = self
            .bus
            .prefix
            .selected_index()
            .unwrap_or((self.cpu.regs.pc, opcode));
        Ok(FetchedInstruction {
            pc,
            opcode,
            cycles: self.kind.t_states(),
        })
    }
    pub fn complete(
        self,
        receipt: WordAddReceipt,
        waits: u32,
        inputs: CycleInterrupts,
    ) -> Result<WordAddCompletion, CycleError> {
        match (self.kind, receipt) {
            (
                WordAddKind::M1 { address, .. },
                WordAddReceipt::M1 {
                    address: a,
                    opcode,
                    known_mask,
                },
            ) => {
                if address != a {
                    return Err(CycleError::SampleMismatch);
                }
                if known_mask != 255 {
                    return Err(CycleError::UnknownData);
                }
                self.cpu
                    .next_physical_cycle(&self.bus)?
                    .validate_interrupts(inputs)?;
                self.instruction(opcode)?;
                let span = 11_u32.checked_add(waits).ok_or(CycleError::ClockOverflow)?;
                self.cpu
                    .cycles
                    .checked_add(u64::from(span))
                    .ok_or(CycleError::ClockOverflow)?;
            }
            (WordAddKind::Internal { .. }, WordAddReceipt::Internal) if waits == 0 => {
                self.tail
                    .as_ref()
                    .ok_or(CycleError::InvalidCompletion)?
                    .validate(&self.cpu, &self.bus)?;
            }
            _ => return Err(CycleError::SampleMismatch),
        }
        Ok(WordAddCompletion {
            request: self,
            receipt,
            waits,
        })
    }
}

impl Cpu {
    pub(super) fn word_add_m1(&self, bus: &BusState) -> Result<WordAddKind, CycleError> {
        if self.state != CpuState::Running
            || self.enable_interrupts_delay > 2
            || bus.transfer.is_some()
        {
            return Err(CycleError::UnsupportedState);
        }
        if !bus.prefix.pending()
            && matches!(self.last_opcode, Z80_PREFIX_CB | Z80_PREFIX_ED)
            && self.instruction_byte_count == 1
            && self.last_m1_fetch_count == 1
            && self.instruction_bytes[0] == self.last_opcode
            && self.regs.pc == self.last_opcode_pc.wrapping_add(1)
        {
            return Err(CycleError::UnsupportedCycle);
        }
        if let Some((pc, prefix)) = bus.prefix.selected_index()
            && (!matches!(prefix, Z80_PREFIX_DD | Z80_PREFIX_FD)
                || self.last_opcode != prefix
                || self.last_opcode_pc != pc
                || self.regs.pc != pc.wrapping_add(1)
                || self.instruction_bytes[0] != prefix
                || self.instruction_byte_count != 1
                || self.last_m1_fetch_count != 1
                || self.instruction_bytes[1..].iter().any(|v| *v != 0))
        {
            return Err(CycleError::UnsupportedCycle);
        }
        let CycleKind::M1 {
            address,
            halted: false,
            refresh_address,
        } = self.next_physical_cycle(bus)?.kind()
        else {
            return Err(CycleError::UnsupportedCycle);
        };
        Ok(WordAddKind::M1 {
            address,
            refresh_address,
        })
    }

    /// Preserve the returned tail with both lagging architectural states until final retirement.
    pub fn next_word_add_cycle(
        &self,
        bus: &BusState,
        tail: Option<&WordAddTail>,
    ) -> Result<WordAddRequest, CycleError> {
        let m1 = self.word_add_m1(bus)?;
        let kind = if let Some(tail) = tail {
            tail.validate(self, bus)?;
            WordAddKind::Internal {
                retained_address: tail.retained_address(),
                phase: tail.phase,
            }
        } else {
            m1
        };
        Ok(WordAddRequest {
            cpu: self.clone(),
            bus: *bus,
            kind,
            tail: tail.cloned(),
        })
    }

    pub fn retire_word_add_cycle_with_evidence(
        &mut self,
        bus: &mut BusState,
        completion: WordAddCompletion,
    ) -> Result<(WordAddRetirement, RetirementEvidence), CycleError> {
        let before = self.cycles;
        let consumed = match completion.receipt {
            WordAddReceipt::M1 {
                address, opcode, ..
            } => Some(ConsumedInstructionByte {
                address,
                value: opcode,
                class: InstructionByteClass::M1,
            }),
            WordAddReceipt::Internal => None,
        };
        let retirement = self.retire_word_add_cycle(bus, completion)?;
        let evidence = RetirementEvidence::new(
            retirement.step.complete,
            consumed,
            before,
            self.cycles,
            RetirementKind::Instruction,
        );
        Ok((retirement, evidence))
    }

    pub fn retire_word_add_cycle(
        &mut self,
        bus: &mut BusState,
        completion: WordAddCompletion,
    ) -> Result<WordAddRetirement, CycleError> {
        let WordAddCompletion {
            request,
            receipt,
            waits,
        } = completion;
        if request.cpu != *self || request.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        if self.next_word_add_cycle(bus, request.tail.as_ref())?.kind != request.kind {
            return Err(CycleError::InvalidCompletion);
        }
        let opcode = match receipt {
            WordAddReceipt::M1 { opcode, .. } => opcode,
            WordAddReceipt::Internal => {
                request
                    .tail
                    .as_ref()
                    .ok_or(CycleError::InvalidCompletion)?
                    .opcode
            }
        };
        let mut instruction = request.instruction(opcode)?;
        instruction.cycles = instruction
            .cycles
            .checked_add(waits)
            .ok_or(CycleError::ClockOverflow)?;
        let tail = match (request.kind, receipt) {
            (
                WordAddKind::M1 { address, .. },
                WordAddReceipt::M1 {
                    address: a,
                    opcode,
                    known_mask: 255,
                },
            ) if address == a => WordAddTail {
                cpu: self.clone(),
                bus: *bus,
                opcode,
                waits,
                phase: WordAddPhase::Internal4,
            },
            (
                WordAddKind::Internal {
                    phase: WordAddPhase::Internal4,
                    ..
                },
                WordAddReceipt::Internal,
            ) if waits == 0 => {
                let mut tail = request.tail.ok_or(CycleError::InvalidCompletion)?;
                tail.phase = WordAddPhase::Internal3;
                tail
            }
            (
                WordAddKind::Internal {
                    phase: WordAddPhase::Internal3,
                    ..
                },
                WordAddReceipt::Internal,
            ) if waits == 0 => {
                let tail = request.tail.ok_or(CycleError::InvalidCompletion)?;
                tail.validate(self, bus)?;
                let accounted = 11_u32
                    .checked_add(tail.waits)
                    .ok_or(CycleError::ClockOverflow)?;
                let (next, after) = word_effect(self, *bus, tail.opcode, tail.waits)?;
                *self = next;
                *bus = after;
                return Ok(WordAddRetirement {
                    step: BoundedStep {
                        instruction,
                        complete: true,
                    },
                    accounted_t_states: accounted,
                    tail: None,
                });
            }
            _ => return Err(CycleError::InvalidCompletion),
        };
        tail.validate(self, bus)?;
        Ok(WordAddRetirement {
            step: BoundedStep {
                instruction,
                complete: false,
            },
            accounted_t_states: 0,
            tail: Some(tail),
        })
    }
}

struct FrozenOpcode {
    address: u16,
    opcode: u8,
    reads: Cell<u8>,
    invalid: Cell<bool>,
}
impl Z80Bus for FrozenOpcode {
    fn cpu_read(&self, address: u16) -> u8 {
        self.invalid
            .set(self.invalid.get() || address != self.address || self.reads.get() != 0);
        self.reads.set(self.reads.get().saturating_add(1));
        self.opcode
    }
    fn cpu_write(&mut self, _: u16, _: u8) {
        self.invalid.set(true);
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

fn word_effect(
    cpu: &Cpu,
    bus: BusState,
    opcode: u8,
    waits: u32,
) -> Result<(Cpu, BusState), CycleError> {
    cpu.word_add_m1(&bus)?;
    let span = 11_u32.checked_add(waits).ok_or(CycleError::ClockOverflow)?;
    let total = cpu
        .cycles
        .checked_add(u64::from(span))
        .ok_or(CycleError::ClockOverflow)?;
    let mut bytes = FrozenOpcode {
        address: cpu.regs.pc,
        opcode,
        reads: Cell::new(0),
        invalid: Cell::new(false),
    };
    let mut next = cpu.clone();
    let mut after = bus;
    let mut result = next
        .step_bus_segment(&mut bytes, &mut after)
        .ok_or(CycleError::InvalidCompletion)?;
    if bytes.invalid.get()
        || bytes.reads.get() != 1
        || !result.complete
        || result.instruction.cycles != 11
        || after.pending()
        || next.trap.is_some()
        || next.last_step_was_interrupt
        || !next.extend_bus_segment(&mut result.instruction, waits)
        || next.cycles != total
    {
        return Err(CycleError::InvalidCompletion);
    }
    Ok((next, after))
}

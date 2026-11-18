use super::*;
use std::cell::Cell;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DjnzPhase {
    Operand,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DjnzKind {
    M1 { address: u16, refresh_address: u16 },
    Operand { address: u16, taken: bool },
    Internal { retained_address: u16 },
}

impl DjnzKind {
    pub fn address(self) -> u16 {
        match self {
            Self::M1 { address, .. } | Self::Operand { address, .. } => address,
            Self::Internal { retained_address } => retained_address,
        }
    }
    pub fn t_states(self) -> u32 {
        if matches!(self, Self::Operand { .. }) {
            3
        } else {
            5
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DjnzReceipt {
    M1 {
        address: u16,
        opcode: u8,
        known_mask: u8,
    },
    Read {
        address: u16,
        value: u8,
        known_mask: u8,
    },
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DjnzTail {
    cpu: Cpu,
    bus: BusState,
    m1_waits: u32,
    operand: Option<(u8, u32)>,
}

impl DjnzTail {
    pub fn phase(&self) -> DjnzPhase {
        if self.operand.is_some() {
            DjnzPhase::Internal
        } else {
            DjnzPhase::Operand
        }
    }
    pub fn opcode(&self) -> u8 {
        0x10
    }
    pub fn m1_waits(&self) -> u32 {
        self.m1_waits
    }
    pub fn operand(&self) -> Option<(u8, u32)> {
        self.operand
    }
    pub fn address(&self) -> u16 {
        self.cpu.regs.pc.wrapping_add(1)
    }
    pub fn refresh_address(&self) -> u16 {
        u16::from_be_bytes([self.cpu.regs.i, self.cpu.regs.r])
    }
    pub fn taken(&self) -> bool {
        self.cpu.regs.b != 1
    }
    pub fn elapsed(&self) -> u64 {
        5 + u64::from(self.m1_waits) + self.operand.map_or(0, |(_, waits)| 3 + u64::from(waits))
    }
    pub fn validate(&self, cpu: &Cpu, bus: &BusState) -> Result<(), CycleError> {
        if self.cpu != *cpu || self.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        cpu.djnz_m1(bus)?;
        if self.operand.is_some() && !self.taken() {
            return Err(CycleError::InvalidCompletion);
        }
        djnz_span(
            cpu,
            self.m1_waits,
            self.operand.map_or(0, |(_, waits)| waits),
        )?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct DjnzRequest {
    cpu: Cpu,
    bus: BusState,
    kind: DjnzKind,
    tail: Option<DjnzTail>,
}

#[derive(Debug)]
pub struct DjnzCompletion {
    request: DjnzRequest,
    receipt: DjnzReceipt,
    waits: u32,
}

#[derive(Debug)]
pub struct DjnzRetirement {
    pub step: BoundedStep,
    pub accounted_t_states: u32,
    pub tail: Option<DjnzTail>,
}

impl DjnzRequest {
    pub fn kind(&self) -> DjnzKind {
        self.kind
    }
    pub fn instruction(&self) -> FetchedInstruction {
        let (pc, opcode) = self
            .bus
            .prefix
            .selected_index()
            .unwrap_or((self.cpu.regs.pc, 0x10));
        FetchedInstruction {
            pc,
            opcode,
            cycles: self.kind.t_states(),
        }
    }
    pub fn complete(
        self,
        receipt: DjnzReceipt,
        waits: u32,
        inputs: CycleInterrupts,
    ) -> Result<DjnzCompletion, CycleError> {
        match (self.kind, receipt) {
            (
                DjnzKind::M1 { address, .. },
                DjnzReceipt::M1 {
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
                if opcode != 0x10 {
                    return Err(CycleError::UnsupportedOpcode {
                        pc: address,
                        opcode,
                    });
                }
                self.cpu
                    .next_physical_cycle(&self.bus)?
                    .validate_interrupts(inputs)?;
                djnz_span(&self.cpu, waits, 0)?;
            }
            (
                DjnzKind::Operand { address, .. },
                DjnzReceipt::Read {
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
                let tail = self.tail.as_ref().ok_or(CycleError::InvalidCompletion)?;
                tail.validate(&self.cpu, &self.bus)?;
                djnz_span(&self.cpu, tail.m1_waits, waits)?;
            }
            (DjnzKind::Internal { .. }, DjnzReceipt::Internal) if waits == 0 => {
                self.tail
                    .as_ref()
                    .ok_or(CycleError::InvalidCompletion)?
                    .validate(&self.cpu, &self.bus)?;
            }
            _ => return Err(CycleError::SampleMismatch),
        }
        Ok(DjnzCompletion {
            request: self,
            receipt,
            waits,
        })
    }
}

impl Cpu {
    fn djnz_m1(&self, bus: &BusState) -> Result<DjnzKind, CycleError> {
        let WordAddKind::M1 {
            address,
            refresh_address,
        } = self.word_add_m1(bus)?
        else {
            return Err(CycleError::UnsupportedCycle);
        };
        Ok(DjnzKind::M1 {
            address,
            refresh_address,
        })
    }

    /// Preserve the tail with both lagging architectural states until final retirement.
    pub fn next_djnz_cycle(
        &self,
        bus: &BusState,
        tail: Option<&DjnzTail>,
    ) -> Result<DjnzRequest, CycleError> {
        let m1 = self.djnz_m1(bus)?;
        let kind = if let Some(tail) = tail {
            tail.validate(self, bus)?;
            match tail.phase() {
                DjnzPhase::Operand => DjnzKind::Operand {
                    address: tail.address(),
                    taken: tail.taken(),
                },
                DjnzPhase::Internal => DjnzKind::Internal {
                    retained_address: tail.address(),
                },
            }
        } else {
            m1
        };
        Ok(DjnzRequest {
            cpu: self.clone(),
            bus: *bus,
            kind,
            tail: tail.cloned(),
        })
    }

    pub fn retire_djnz_cycle_with_evidence(
        &mut self,
        bus: &mut BusState,
        completion: DjnzCompletion,
    ) -> Result<(DjnzRetirement, RetirementEvidence), CycleError> {
        let before = self.cycles;
        let consumed = match completion.receipt {
            DjnzReceipt::M1 {
                address, opcode, ..
            } => Some(ConsumedInstructionByte {
                address,
                value: opcode,
                class: InstructionByteClass::M1,
            }),
            DjnzReceipt::Read { address, value, .. } => Some(ConsumedInstructionByte {
                address,
                value,
                class: InstructionByteClass::Displacement,
            }),
            DjnzReceipt::Internal => None,
        };
        let retirement = self.retire_djnz_cycle(bus, completion)?;
        let evidence = RetirementEvidence::new(
            retirement.step.complete,
            consumed,
            before,
            self.cycles,
            RetirementKind::Instruction,
        );
        Ok((retirement, evidence))
    }

    pub fn retire_djnz_cycle(
        &mut self,
        bus: &mut BusState,
        completion: DjnzCompletion,
    ) -> Result<DjnzRetirement, CycleError> {
        let DjnzCompletion {
            request,
            receipt,
            waits,
        } = completion;
        if request.cpu != *self || request.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        if self.next_djnz_cycle(bus, request.tail.as_ref())?.kind != request.kind {
            return Err(CycleError::InvalidCompletion);
        }
        let mut instruction = request.instruction();
        instruction.cycles = instruction
            .cycles
            .checked_add(waits)
            .ok_or(CycleError::ClockOverflow)?;
        let (tail, finish) = match (request.kind, receipt) {
            (
                DjnzKind::M1 { address, .. },
                DjnzReceipt::M1 {
                    address: a,
                    opcode: 0x10,
                    known_mask: 255,
                },
            ) if address == a => (
                DjnzTail {
                    cpu: self.clone(),
                    bus: *bus,
                    m1_waits: waits,
                    operand: None,
                },
                false,
            ),
            (
                DjnzKind::Operand { address, taken },
                DjnzReceipt::Read {
                    address: a,
                    value,
                    known_mask: 255,
                },
            ) if address == a => {
                let mut tail = request.tail.ok_or(CycleError::InvalidCompletion)?;
                tail.operand = Some((value, waits));
                (tail, !taken)
            }
            (DjnzKind::Internal { .. }, DjnzReceipt::Internal) if waits == 0 => {
                (request.tail.ok_or(CycleError::InvalidCompletion)?, true)
            }
            _ => return Err(CycleError::InvalidCompletion),
        };
        if finish {
            let (value, read_waits) = tail.operand.ok_or(CycleError::InvalidCompletion)?;
            let accounted = djnz_span(self, tail.m1_waits, read_waits)?;
            let (next, after) = djnz_effect(self, *bus, value, accounted)?;
            *self = next;
            *bus = after;
            return Ok(DjnzRetirement {
                step: BoundedStep {
                    instruction,
                    complete: true,
                },
                accounted_t_states: accounted,
                tail: None,
            });
        }
        tail.validate(self, bus)?;
        Ok(DjnzRetirement {
            step: BoundedStep {
                instruction,
                complete: false,
            },
            accounted_t_states: 0,
            tail: Some(tail),
        })
    }
}

fn djnz_span(cpu: &Cpu, m1_waits: u32, read_waits: u32) -> Result<u32, CycleError> {
    let base: u32 = if cpu.regs.b == 1 { 8 } else { 13 };
    let span = base
        .checked_add(m1_waits)
        .and_then(|v| v.checked_add(read_waits))
        .ok_or(CycleError::ClockOverflow)?;
    cpu.cycles
        .checked_add(u64::from(span))
        .ok_or(CycleError::ClockOverflow)?;
    Ok(span)
}

struct FrozenBytes {
    address: u16,
    displacement: u8,
    reads: Cell<u8>,
    invalid: Cell<bool>,
}
impl Z80Bus for FrozenBytes {
    fn cpu_read(&self, address: u16) -> u8 {
        let count = self.reads.get();
        self.invalid.set(
            self.invalid.get()
                || count > 1
                || address != self.address.wrapping_add(u16::from(count)),
        );
        self.reads.set(count.saturating_add(1));
        if count == 0 { 0x10 } else { self.displacement }
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

fn djnz_effect(
    cpu: &Cpu,
    bus: BusState,
    displacement: u8,
    accounted: u32,
) -> Result<(Cpu, BusState), CycleError> {
    cpu.djnz_m1(&bus)?;
    let base = if cpu.regs.b == 1 { 8 } else { 13 };
    let waits = accounted
        .checked_sub(base)
        .ok_or(CycleError::InvalidCompletion)?;
    let total = cpu
        .cycles
        .checked_add(u64::from(accounted))
        .ok_or(CycleError::ClockOverflow)?;
    let mut bytes = FrozenBytes {
        address: cpu.regs.pc,
        displacement,
        reads: Cell::new(0),
        invalid: Cell::new(false),
    };
    let mut next = cpu.clone();
    let mut after = bus;
    let mut result = next
        .step_bus_segment_legacy_djnz(&mut bytes, &mut after)
        .ok_or(CycleError::InvalidCompletion)?;
    if bytes.invalid.get()
        || bytes.reads.get() != 2
        || !result.complete
        || result.instruction.cycles != base
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

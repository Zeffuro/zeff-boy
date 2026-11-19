use super::*;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JrCycleKind {
    Displacement { address: u16, taken: bool },
    Internal { retained_address: u16 },
}

impl JrCycleKind {
    pub fn address(self) -> u16 {
        match self {
            Self::Displacement { address, .. } => address,
            Self::Internal { retained_address } => retained_address,
        }
    }
    pub fn t_states(self) -> u32 {
        match self {
            Self::Displacement { .. } => 3,
            Self::Internal { .. } => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JrReceipt {
    Read {
        address: u16,
        value: u8,
        known_mask: u8,
    },
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JrTail {
    cpu: Cpu,
    bus: BusState,
    address: u16,
    displacement: u8,
    waits: u32,
}

impl JrTail {
    pub fn address(&self) -> u16 {
        self.address
    }
    pub fn displacement(&self) -> u8 {
        self.displacement
    }
    pub fn read_waits(&self) -> u32 {
        self.waits
    }
    pub fn validate(&self, cpu: &Cpu, bus: &BusState) -> Result<(), CycleError> {
        if self.cpu != *cpu || self.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        let (address, taken) = cpu.jr_context(bus)?;
        if address != self.address || !taken {
            return Err(CycleError::InvalidCompletion);
        }
        let span = 8_u32
            .checked_add(self.waits)
            .ok_or(CycleError::ClockOverflow)?;
        cpu.cycles
            .checked_add(u64::from(span))
            .ok_or(CycleError::ClockOverflow)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct JrCycleRequest {
    cpu: Cpu,
    bus: BusState,
    kind: JrCycleKind,
    tail: Option<JrTail>,
}

#[derive(Debug)]
pub struct JrCycleCompletion {
    request: JrCycleRequest,
    receipt: JrReceipt,
    waits: u32,
}

#[derive(Debug)]
pub struct JrRetirement {
    pub step: BoundedStep,
    pub accounted_t_states: u32,
    pub tail: Option<JrTail>,
}

impl JrCycleRequest {
    pub fn kind(&self) -> JrCycleKind {
        self.kind
    }
    pub fn instruction(&self) -> FetchedInstruction {
        FetchedInstruction {
            pc: self.cpu.last_opcode_pc,
            opcode: self.cpu.last_opcode,
            cycles: self.kind.t_states(),
        }
    }
    pub fn complete(self, receipt: JrReceipt, waits: u32) -> Result<JrCycleCompletion, CycleError> {
        match (self.kind, receipt) {
            (
                JrCycleKind::Displacement { address, .. },
                JrReceipt::Read {
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
                let (_, taken) = self.cpu.jr_context(&self.bus)?;
                let span = (if taken { 8_u32 } else { 3 })
                    .checked_add(waits)
                    .ok_or(CycleError::ClockOverflow)?;
                self.cpu
                    .cycles
                    .checked_add(u64::from(span))
                    .ok_or(CycleError::ClockOverflow)?;
            }
            (JrCycleKind::Internal { .. }, JrReceipt::Internal) if waits == 0 => {
                self.tail
                    .as_ref()
                    .ok_or(CycleError::InvalidCompletion)?
                    .validate(&self.cpu, &self.bus)?;
            }
            _ => return Err(CycleError::SampleMismatch),
        }
        Ok(JrCycleCompletion {
            request: self,
            receipt,
            waits,
        })
    }
}

impl Cpu {
    fn jr_context(&self, bus: &BusState) -> Result<(u16, bool), CycleError> {
        if self.state != CpuState::Running
            || self.trap.is_some()
            || self.last_step_was_interrupt
            || self.enable_interrupts_delay > 2
        {
            return Err(CycleError::UnsupportedState);
        }
        let Some(Transfer {
            pc,
            operation: Operation::Single { opcode, index },
        }) = bus.transfer
        else {
            return Err(CycleError::UnsupportedCycle);
        };
        if !matches!(opcode, 0x18 | 0x20 | 0x28 | 0x30 | 0x38) {
            return Err(CycleError::UnsupportedCycle);
        }
        let count = if let Some(prefix) = index {
            if !matches!(prefix, Z80_PREFIX_DD | Z80_PREFIX_FD)
                || bus.prefix.selected_index() != Some((pc, prefix))
                || self.last_opcode != prefix
                || self.instruction_bytes[0] != prefix
                || self.instruction_bytes[1] != opcode
            {
                return Err(CycleError::UnsupportedCycle);
            }
            2
        } else {
            if bus.prefix.pending()
                || self.last_opcode != opcode
                || self.instruction_bytes[0] != opcode
            {
                return Err(CycleError::UnsupportedCycle);
            }
            1
        };
        if self.last_opcode_pc != pc
            || self.instruction_byte_count != count
            || self.last_m1_fetch_count != count
            || self.regs.pc != pc.wrapping_add(u16::from(count))
            || self.instruction_bytes[usize::from(count)..]
                .iter()
                .any(|v| *v != 0)
        {
            return Err(CycleError::UnsupportedCycle);
        }
        Ok((
            self.regs.pc,
            opcode == 0x18 || self.condition_is_true((opcode >> 3) & 3),
        ))
    }

    /// A taken read leaves this architectural checkpoint unchanged; retain its tail with both states.
    pub fn next_jr_cycle(
        &self,
        bus: &BusState,
        tail: Option<&JrTail>,
    ) -> Result<JrCycleRequest, CycleError> {
        let (address, taken) = self.jr_context(bus)?;
        let kind = if let Some(tail) = tail {
            tail.validate(self, bus)?;
            JrCycleKind::Internal {
                retained_address: address,
            }
        } else {
            JrCycleKind::Displacement { address, taken }
        };
        Ok(JrCycleRequest {
            cpu: self.clone(),
            bus: *bus,
            kind,
            tail: tail.cloned(),
        })
    }

    pub fn retire_jr_cycle_with_evidence(
        &mut self,
        bus: &mut BusState,
        completion: JrCycleCompletion,
    ) -> Result<(JrRetirement, RetirementEvidence), CycleError> {
        let before = self.cycles;
        let consumed = match (completion.request.kind, completion.receipt) {
            (JrCycleKind::Displacement { .. }, JrReceipt::Read { address, value, .. }) => {
                Some(ConsumedInstructionByte {
                    address,
                    value,
                    class: InstructionByteClass::Displacement,
                })
            }
            _ => None,
        };
        let retirement = self.retire_jr_cycle(bus, completion)?;
        let evidence = RetirementEvidence::new(
            retirement.step.complete,
            consumed,
            before,
            self.cycles,
            RetirementKind::Instruction,
        );
        Ok((retirement, evidence))
    }

    pub fn retire_jr_cycle(
        &mut self,
        bus: &mut BusState,
        completion: JrCycleCompletion,
    ) -> Result<JrRetirement, CycleError> {
        let JrCycleCompletion {
            request,
            receipt,
            waits,
        } = completion;
        if request.cpu != *self || request.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        if self.next_jr_cycle(bus, request.tail.as_ref())?.kind != request.kind {
            return Err(CycleError::InvalidCompletion);
        }
        let physical = request
            .kind
            .t_states()
            .checked_add(waits)
            .ok_or(CycleError::ClockOverflow)?;
        let mut instruction = request.instruction();
        instruction.cycles = physical;
        let (value, taken, accounted) = match (request.kind, receipt) {
            (
                JrCycleKind::Displacement { address, taken },
                JrReceipt::Read {
                    address: a,
                    value,
                    known_mask: 255,
                },
            ) if address == a => {
                if taken {
                    let tail = JrTail {
                        cpu: self.clone(),
                        bus: *bus,
                        address,
                        displacement: value,
                        waits,
                    };
                    tail.validate(self, bus)?;
                    return Ok(JrRetirement {
                        step: BoundedStep {
                            instruction,
                            complete: false,
                        },
                        accounted_t_states: 0,
                        tail: Some(tail),
                    });
                }
                (value, false, physical)
            }
            (JrCycleKind::Internal { .. }, JrReceipt::Internal) if waits == 0 => {
                let tail = request.tail.ok_or(CycleError::InvalidCompletion)?;
                tail.validate(self, bus)?;
                (
                    tail.displacement,
                    true,
                    8_u32
                        .checked_add(tail.waits)
                        .ok_or(CycleError::ClockOverflow)?,
                )
            }
            _ => return Err(CycleError::InvalidCompletion),
        };
        let total = self
            .cycles
            .checked_add(u64::from(accounted))
            .ok_or(CycleError::ClockOverflow)?;
        let mut next = self.clone();
        next.accept_fetched_byte(value);
        if taken {
            next.apply_relative_jump(value as i8);
        }
        next.finish_instruction(accounted);
        if next.cycles != total {
            return Err(CycleError::InvalidCompletion);
        }
        *self = next;
        *bus = BusState::default();
        Ok(JrRetirement {
            step: BoundedStep {
                instruction,
                complete: true,
            },
            accounted_t_states: accounted,
            tail: None,
        })
    }
}

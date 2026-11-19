use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyCycleKind {
    Read { address: u16 },
    Write { address: u16, value: u8 },
    Repeat { retained_address: u16 },
}
impl CopyCycleKind {
    pub fn address(self) -> u16 {
        match self {
            Self::Read { address } | Self::Write { address, .. } => address,
            Self::Repeat { retained_address } => retained_address,
        }
    }
    pub fn t_states(self) -> u32 {
        if matches!(self, Self::Read { .. }) {
            3
        } else {
            5
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyReceipt {
    Read { value: u8, known: u8 },
    Write,
    Repeat,
}
#[derive(Debug)]
pub struct CopyCycleRequest {
    cpu: Cpu,
    bus: BusState,
    kind: CopyCycleKind,
}
#[derive(Debug)]
pub struct CopyCycleCompletion {
    request: CopyCycleRequest,
    receipt: CopyReceipt,
    waits: u32,
}
impl CopyCycleRequest {
    pub fn kind(&self) -> CopyCycleKind {
        self.kind
    }
    pub fn instruction(&self) -> FetchedInstruction {
        FetchedInstruction {
            pc: self.cpu.last_opcode_pc,
            opcode: self.cpu.last_opcode,
            cycles: self.kind.t_states(),
        }
    }
    pub fn captured_byte(&self) -> Option<u8> {
        let Some(Transfer {
            operation: Operation::EdBlock(block),
            ..
        }) = self.bus.transfer
        else {
            unreachable!()
        };
        block.copy_byte()
    }
    pub fn finishes(&self) -> bool {
        let Some(Transfer {
            operation: Operation::EdBlock(block),
            ..
        }) = self.bus.transfer
        else {
            unreachable!()
        };
        block.copy_finishes(&self.cpu)
    }
    pub fn complete(
        self,
        receipt: CopyReceipt,
        waits: u32,
    ) -> Result<CopyCycleCompletion, CycleError> {
        match (self.kind, receipt) {
            (CopyCycleKind::Read { .. }, CopyReceipt::Read { known: 255, .. })
            | (CopyCycleKind::Write { .. }, CopyReceipt::Write) => {}
            (CopyCycleKind::Repeat { .. }, CopyReceipt::Repeat) if waits == 0 => {}
            _ => return Err(CycleError::SampleMismatch),
        }
        let cycles = self
            .kind
            .t_states()
            .checked_add(waits)
            .ok_or(CycleError::ClockOverflow)?;
        self.cpu
            .cycles
            .checked_add(u64::from(cycles))
            .ok_or(CycleError::ClockOverflow)?;
        Ok(CopyCycleCompletion {
            request: self,
            receipt,
            waits,
        })
    }
}
impl Cpu {
    pub fn next_copy_cycle(&self, bus: &BusState) -> Result<CopyCycleRequest, CycleError> {
        if self.state != CpuState::Running
            || self.trap.is_some()
            || self.last_step_was_interrupt
            || self.enable_interrupts_delay > 2
            || bus.prefix.pending()
        {
            return Err(CycleError::UnsupportedState);
        }
        let Some(Transfer {
            pc,
            operation: Operation::EdBlock(block),
        }) = bus.transfer
        else {
            return Err(CycleError::UnsupportedCycle);
        };
        if pc != self.last_opcode_pc {
            return Err(CycleError::UnsupportedCycle);
        }
        let kind = block.copy_kind(self)?;
        Ok(CopyCycleRequest {
            cpu: self.clone(),
            bus: *bus,
            kind,
        })
    }

    pub fn retire_copy_cycle_with_evidence(
        &mut self,
        bus: &mut BusState,
        completion: CopyCycleCompletion,
    ) -> Result<(BoundedStep, RetirementEvidence), CycleError> {
        let before = self.cycles;
        let consumed = None;
        let retirement = self.retire_copy_cycle(bus, completion)?;
        let evidence = RetirementEvidence::new(
            retirement.complete,
            consumed,
            before,
            self.cycles,
            RetirementKind::Instruction,
        );
        Ok((retirement, evidence))
    }

    pub fn retire_copy_cycle(
        &mut self,
        bus: &mut BusState,
        completion: CopyCycleCompletion,
    ) -> Result<BoundedStep, CycleError> {
        let CopyCycleCompletion {
            request,
            receipt,
            waits,
        } = completion;
        if request.cpu != *self || request.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        if self.next_copy_cycle(bus)?.kind != request.kind {
            return Err(CycleError::InvalidCompletion);
        }
        let cycles = request
            .kind
            .t_states()
            .checked_add(waits)
            .ok_or(CycleError::ClockOverflow)?;
        let total = self
            .cycles
            .checked_add(u64::from(cycles))
            .ok_or(CycleError::ClockOverflow)?;
        let mut next = self.clone();
        let Some(Transfer {
            pc,
            operation: Operation::EdBlock(block),
        }) = bus.transfer
        else {
            return Err(CycleError::InvalidCompletion);
        };
        let after = block.complete_copy(&mut next, receipt)?;
        if after.is_none() {
            next.finish_instruction(cycles);
        } else {
            next.cycles = total;
        }
        if next.cycles != total {
            return Err(CycleError::InvalidCompletion);
        }
        let mut instruction = request.instruction();
        instruction.cycles = cycles;
        *self = next;
        *bus = BusState {
            prefix: PrefixState::default(),
            transfer: after.map(|block| Transfer {
                pc,
                operation: Operation::EdBlock(block),
            }),
        };
        Ok(BoundedStep {
            instruction,
            complete: after.is_none(),
        })
    }
}

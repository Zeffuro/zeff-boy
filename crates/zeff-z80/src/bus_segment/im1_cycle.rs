use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Im1CycleKind {
    Acknowledge {
        pc: u16,
        refresh: u16,
    },
    StackWrite {
        address: u16,
        value: u8,
        final_write: bool,
    },
}

impl Im1CycleKind {
    pub fn t_states(self) -> u32 {
        match self {
            Self::Acknowledge { .. } => 7,
            Self::StackWrite { .. } => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Im1Receipt {
    Acknowledged,
    Written { address: u16, value: u8 },
}

#[derive(Debug)]
pub struct Im1CycleRequest {
    cpu: Cpu,
    bus: BusState,
    kind: Im1CycleKind,
    accepted: bool,
}

#[derive(Debug)]
pub struct Im1CycleCompletion {
    request: Im1CycleRequest,
    waits: u32,
}

impl Im1CycleRequest {
    pub fn kind(&self) -> Im1CycleKind {
        self.kind
    }

    pub fn complete(
        self,
        receipt: Im1Receipt,
        waits: u32,
    ) -> Result<Im1CycleCompletion, CycleError> {
        let valid = match (self.kind, receipt) {
            (Im1CycleKind::Acknowledge { .. }, Im1Receipt::Acknowledged) => true,
            (
                Im1CycleKind::StackWrite { address, value, .. },
                Im1Receipt::Written {
                    address: a,
                    value: v,
                },
            ) => address == a && value == v,
            _ => false,
        };
        if !valid {
            return Err(CycleError::SampleMismatch);
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
        Ok(Im1CycleCompletion {
            request: self,
            waits,
        })
    }
}

impl BusState {
    pub fn interrupt_pending(&self) -> bool {
        matches!(
            self.transfer.map(|t| t.operation),
            Some(Operation::Interrupt(_))
        )
    }
}

impl Cpu {
    /// The caller owns the accepted instruction-boundary IRQ and physical ACK/write edges.
    pub fn next_im1_cycle(
        &self,
        bus: &BusState,
        accepted: bool,
    ) -> Result<Im1CycleRequest, CycleError> {
        if self.state == CpuState::Suspended
            || self.trap.is_some()
            || self.interrupt_mode != InterruptMode::Im1
            || self.enable_interrupts_delay != 0
            || bus.prefix.pending()
        {
            return Err(CycleError::UnsupportedInterrupt);
        }
        let kind = match bus.transfer {
            None if accepted && self.interrupt_flip_flop_1 => Im1CycleKind::Acknowledge {
                pc: self.regs.pc,
                refresh: u16::from_be_bytes([self.regs.i, self.regs.r]),
            },
            Some(Transfer {
                pc,
                operation: Operation::Interrupt(service),
            }) if !accepted && pc == service.pc() && pc == self.regs.pc => {
                let (address, value, final_write) = service
                    .im1_write(self)
                    .ok_or(CycleError::UnsupportedCycle)?;
                Im1CycleKind::StackWrite {
                    address,
                    value,
                    final_write,
                }
            }
            _ => return Err(CycleError::UnsupportedCycle),
        };
        Ok(Im1CycleRequest {
            cpu: self.clone(),
            bus: *bus,
            kind,
            accepted,
        })
    }

    pub fn retire_im1_cycle_with_evidence(
        &mut self,
        bus: &mut BusState,
        completion: Im1CycleCompletion,
    ) -> Result<(BoundedStep, RetirementEvidence), CycleError> {
        let before = self.cycles;
        let consumed = None;
        let retirement = self.retire_im1_cycle(bus, completion)?;
        let evidence = RetirementEvidence::new(
            retirement.complete,
            consumed,
            before,
            self.cycles,
            RetirementKind::Interrupt,
        );
        Ok((retirement, evidence))
    }

    pub fn retire_im1_cycle(
        &mut self,
        bus: &mut BusState,
        completion: Im1CycleCompletion,
    ) -> Result<BoundedStep, CycleError> {
        let Im1CycleCompletion { request, waits } = completion;
        if *self != request.cpu || *bus != request.bus {
            return Err(CycleError::StaleContinuation);
        }
        if self.next_im1_cycle(bus, request.accepted)?.kind != request.kind {
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
        let pc = self.regs.pc;
        let mut next = self.clone();
        let mut state = *bus;
        let service = match request.kind {
            Im1CycleKind::Acknowledge { .. } => {
                next.clear_step_observation();
                Some(interrupt::Service::acknowledge_im1(&mut next))
            }
            Im1CycleKind::StackWrite { .. } => {
                let Some(Transfer {
                    operation: Operation::Interrupt(service),
                    ..
                }) = state.transfer
                else {
                    return Err(CycleError::InvalidCompletion);
                };
                service
                    .complete_im1_write(&mut next)
                    .map_err(|_| CycleError::InvalidCompletion)?
            }
        };
        state.transfer = service.map(Transfer::interrupt);
        next.cycles = total;
        let step = BoundedStep {
            instruction: FetchedInstruction {
                pc,
                opcode: Z80_INTERRUPT_ACK_OPCODE,
                cycles,
            },
            complete: service.is_none(),
        };
        *self = next;
        *bus = state;
        Ok(step)
    }
}

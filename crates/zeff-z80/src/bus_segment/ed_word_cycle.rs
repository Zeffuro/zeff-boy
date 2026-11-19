use super::*;
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdWordPhase {
    Internal4,
    Internal3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdWordKind {
    pub phase: EdWordPhase,
    pub retained_address: u16,
}

impl EdWordKind {
    pub fn t_states(self) -> u32 {
        match self.phase {
            EdWordPhase::Internal4 => 4,
            EdWordPhase::Internal3 => 3,
        }
    }
    pub fn finishes(self) -> bool {
        self.phase == EdWordPhase::Internal3
    }
}

#[derive(Debug)]
pub struct EdWordRequest {
    cpu: Cpu,
    bus: BusState,
    kind: EdWordKind,
}

#[derive(Debug)]
pub struct EdWordCompletion {
    request: EdWordRequest,
}

impl EdWordRequest {
    pub fn kind(&self) -> EdWordKind {
        self.kind
    }
    pub fn instruction(&self) -> FetchedInstruction {
        FetchedInstruction {
            pc: self.cpu.last_opcode_pc,
            opcode: Z80_PREFIX_ED,
            cycles: self.kind.t_states(),
        }
    }
    pub fn complete(self, waits: u32) -> Result<EdWordCompletion, CycleError> {
        if waits != 0 {
            return Err(CycleError::SampleMismatch);
        }
        self.cpu
            .cycles
            .checked_add(u64::from(self.kind.t_states()))
            .ok_or(CycleError::ClockOverflow)?;
        Ok(EdWordCompletion { request: self })
    }
}

impl BusState {
    pub fn ed_word_pending(&self) -> bool {
        matches!(self.transfer, Some(Transfer { operation: Operation::EdInternal(operation), .. }) if operation.word_pending())
    }
}

impl Cpu {
    pub fn next_ed_word_cycle(&self, bus: &BusState) -> Result<EdWordRequest, CycleError> {
        if self.state != CpuState::Running
            || self.trap.is_some()
            || self.last_step_was_interrupt
            || self.enable_interrupts_delay > 2
        {
            return Err(CycleError::UnsupportedState);
        }
        let Some(Transfer {
            pc,
            operation: Operation::EdInternal(operation),
        }) = bus.transfer
        else {
            return Err(CycleError::UnsupportedCycle);
        };
        if bus.prefix.pending()
            || self.last_opcode_pc != pc
            || self.last_opcode != Z80_PREFIX_ED
            || self.instruction_byte_count != 2
            || self.last_m1_fetch_count != 2
            || self.instruction_bytes[0] != Z80_PREFIX_ED
            || self.instruction_bytes[2..].iter().any(|byte| *byte != 0)
            || self.regs.pc != pc.wrapping_add(2)
        {
            return Err(CycleError::UnsupportedCycle);
        }
        let final_phase = operation
            .word_phase(self.instruction_bytes[1])
            .ok_or(CycleError::UnsupportedCycle)?;
        Ok(EdWordRequest {
            cpu: self.clone(),
            bus: *bus,
            kind: EdWordKind {
                phase: if final_phase {
                    EdWordPhase::Internal3
                } else {
                    EdWordPhase::Internal4
                },
                retained_address: u16::from_be_bytes([
                    self.regs.i,
                    (self.regs.r & 0x80) | (self.regs.r.wrapping_sub(1) & 0x7f),
                ]),
            },
        })
    }

    pub fn retire_ed_word_cycle_with_evidence(
        &mut self,
        bus: &mut BusState,
        completion: EdWordCompletion,
    ) -> Result<(BoundedStep, RetirementEvidence), CycleError> {
        let before = self.cycles;
        let consumed = None;
        let retirement = self.retire_ed_word_cycle(bus, completion)?;
        let evidence = RetirementEvidence::new(
            retirement.complete,
            consumed,
            before,
            self.cycles,
            RetirementKind::Instruction,
        );
        Ok((retirement, evidence))
    }

    pub fn retire_ed_word_cycle(
        &mut self,
        bus: &mut BusState,
        completion: EdWordCompletion,
    ) -> Result<BoundedStep, CycleError> {
        let request = completion.request;
        if request.cpu != *self || request.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        if self.next_ed_word_cycle(bus)?.kind != request.kind {
            return Err(CycleError::InvalidCompletion);
        }
        let total = self
            .cycles
            .checked_add(u64::from(request.kind.t_states()))
            .ok_or(CycleError::ClockOverflow)?;
        let mut next = self.clone();
        let mut after = *bus;
        let mut frozen = NoCallbacks(Cell::new(false));
        let step = next
            .step_bus_segment(&mut frozen, &mut after)
            .ok_or(CycleError::InvalidCompletion)?;
        if frozen.0.get()
            || step.instruction != request.instruction()
            || step.complete != request.kind.finishes()
            || next.cycles != total
            || step.complete && after.pending()
            || next.last_step_was_interrupt
            || next.trap.is_some()
        {
            return Err(CycleError::InvalidCompletion);
        }
        if !step.complete {
            next.next_ed_word_cycle(&after)?;
        }
        *self = next;
        *bus = after;
        Ok(step)
    }
}

struct NoCallbacks(Cell<bool>);
impl Z80Bus for NoCallbacks {
    fn cpu_read(&self, _: u16) -> u8 {
        self.0.set(true);
        0
    }
    fn cpu_write(&mut self, _: u16, _: u8) {
        self.0.set(true);
    }
    fn io_read(&mut self, _: u8) -> u8 {
        self.0.set(true);
        0
    }
    fn io_write(&mut self, _: u8, _: u8) {
        self.0.set(true);
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.0.set(true);
        false
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        self.0.set(true);
        false
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.0.set(true);
        false
    }
    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        self.0.set(true);
        0
    }
}

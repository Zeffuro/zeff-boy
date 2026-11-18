use super::*;

#[path = "physical_cycle/sample.rs"]
mod sample;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleKind {
    M1 {
        address: u16,
        halted: bool,
        refresh_address: u16,
    },
    MemoryRead {
        address: u16,
    },
    MemoryReadExtended {
        address: u16,
    },
    MemoryWrite {
        address: u16,
        value: u8,
    },
    MemoryWriteExtended {
        address: u16,
        value: u8,
    },
}

impl CycleKind {
    pub fn address(self) -> u16 {
        match self {
            Self::M1 { address, .. }
            | Self::MemoryRead { address }
            | Self::MemoryReadExtended { address }
            | Self::MemoryWrite { address, .. }
            | Self::MemoryWriteExtended { address, .. } => address,
        }
    }

    pub fn is_m1(self) -> bool {
        matches!(self, Self::M1 { .. })
    }

    pub fn is_write(self) -> bool {
        matches!(
            self,
            Self::MemoryWrite { .. } | Self::MemoryWriteExtended { .. }
        )
    }

    pub fn t_states(self) -> Option<u32> {
        match self {
            Self::M1 { .. } => None,
            Self::MemoryReadExtended { .. } => Some(CYCLES_MEMORY_RMW_READ),
            Self::MemoryWriteExtended { .. } => Some(CYCLES_EXCHANGE_WRITE_LOW),
            Self::MemoryRead { .. } | Self::MemoryWrite { .. } => Some(CYCLES_MEMORY_TRANSFER),
        }
    }

    pub fn minimum_t_states(self) -> u32 {
        self.t_states().unwrap_or(CYCLES_M1)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CycleInterrupts {
    pub maskable_pending: bool,
    pub nmi_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CycleSample {
    pub address: u16,
    pub m1: bool,
    pub write: bool,
    pub value: u8,
    pub known_mask: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleError {
    UnsupportedState,
    UnsupportedCycle,
    UnsupportedOpcode { pc: u16, opcode: u8 },
    UnsupportedInterrupt,
    UnknownData,
    SampleMismatch,
    StaleContinuation,
    ClockOverflow,
    InvalidCompletion,
}

#[derive(Debug)]
pub struct CycleRequest {
    cpu: Cpu,
    bus: BusState,
    kind: CycleKind,
}

#[derive(Debug)]
pub struct CycleCompletion {
    request: CycleRequest,
    sample: CycleSample,
    tw: u32,
    interrupts: CycleInterrupts,
    base_t_states: u32,
}

impl CycleRequest {
    pub fn kind(&self) -> CycleKind {
        self.kind
    }

    pub fn validate_interrupts(&self, inputs: CycleInterrupts) -> Result<(), CycleError> {
        if self.bus.pending() {
            return Ok(());
        }
        if inputs.nmi_pending
            || (inputs.maskable_pending
                && self.cpu.interrupt_flip_flop_1
                && self.cpu.enable_interrupts_delay == 0)
        {
            return Err(CycleError::UnsupportedInterrupt);
        }
        Ok(())
    }

    pub fn complete(
        self,
        sample: CycleSample,
        tw: u32,
        interrupts: CycleInterrupts,
    ) -> Result<CycleCompletion, CycleError> {
        self.validate_interrupts(interrupts)?;
        let base_t_states = self.sampled_t_states(sample)?;
        Ok(CycleCompletion {
            request: self,
            sample,
            tw,
            interrupts,
            base_t_states,
        })
    }
}

impl Cpu {
    /// The caller must qualify RESET at a quiescent machine-cycle boundary.
    pub fn apply_qualified_reset(&mut self, bus: &mut BusState) -> Result<(), CycleError> {
        if self.state == CpuState::Suspended || self.trap.is_some() {
            return Err(CycleError::UnsupportedState);
        }
        let registers = self.regs;
        let shadow = self.shadow;
        let cycles = self.cycles;
        self.reset();
        self.regs = Registers {
            pc: Z80_RESET_PC,
            i: 0,
            r: 0,
            ..registers
        };
        self.shadow = shadow;
        self.cycles = cycles;
        self.last_opcode_pc = Z80_RESET_PC;
        *bus = BusState::default();
        Ok(())
    }

    pub fn next_physical_cycle(&self, bus: &BusState) -> Result<CycleRequest, CycleError> {
        if self.state == CpuState::Suspended || self.trap.is_some() {
            return Err(CycleError::UnsupportedState);
        }
        if !bus.pending()
            && self.state == CpuState::Running
            && matches!(self.last_opcode, Z80_PREFIX_DD | Z80_PREFIX_FD)
            && self.instruction_byte_count == 1
            && self.last_m1_fetch_count == 1
            && self.instruction_bytes[0] == self.last_opcode
            && self.regs.pc == self.last_opcode_pc.wrapping_add(1)
        {
            return Err(CycleError::UnsupportedCycle);
        }
        let kind = if bus.prefix.pending() {
            if self.state != CpuState::Running {
                return Err(CycleError::UnsupportedCycle);
            }
            self.physical_index_cycle(bus)
                .ok_or(CycleError::UnsupportedCycle)?
        } else {
            match bus.transfer {
                None => CycleKind::M1 {
                    address: self.regs.pc,
                    halted: self.state == CpuState::Halted,
                    refresh_address: u16::from_be_bytes([self.regs.i, self.regs.r]),
                },
                Some(Transfer {
                    operation: Operation::Load(load),
                    ..
                }) if self.state == CpuState::Running => {
                    match self
                        .physical_load(bus.transfer.ok_or(CycleError::UnsupportedCycle)?, load)
                        .ok_or(CycleError::UnsupportedCycle)?
                    {
                        load::LoadCycle::Fetch { address } | load::LoadCycle::Read { address } => {
                            CycleKind::MemoryRead { address }
                        }
                        load::LoadCycle::Write { address, value } => {
                            CycleKind::MemoryWrite { address, value }
                        }
                    }
                }
                Some(transfer)
                    if self.state == CpuState::Running
                        && self.physical_prefix(transfer).is_some() =>
                {
                    CycleKind::M1 {
                        address: self.regs.pc,
                        halted: false,
                        refresh_address: u16::from_be_bytes([self.regs.i, self.regs.r]),
                    }
                }
                Some(Transfer {
                    operation:
                        Operation::Single {
                            opcode,
                            index: None,
                        },
                    ..
                }) if self.state == CpuState::Running && opcode == self.last_opcode => {
                    let operand = operand_cycle::Operand::decode(self, opcode)
                        .ok_or(CycleError::UnsupportedCycle)?;
                    match operand
                        .physical_cycle(self, opcode)
                        .ok_or(CycleError::UnsupportedCycle)?
                    {
                        load::LoadCycle::Fetch { address } | load::LoadCycle::Read { address } => {
                            CycleKind::MemoryRead { address }
                        }
                        load::LoadCycle::Write { address, value } => {
                            CycleKind::MemoryWrite { address, value }
                        }
                    }
                }
                Some(Transfer {
                    operation: Operation::Stack(operation),
                    ..
                }) if self.state == CpuState::Running => self
                    .physical_stack(bus.transfer.ok_or(CycleError::UnsupportedCycle)?, operation)
                    .ok_or(CycleError::UnsupportedCycle)?,
                Some(
                    transfer @ Transfer {
                        operation: Operation::Rmw(operation),
                        ..
                    },
                ) if self.state == CpuState::Running => self
                    .physical_rmw(transfer, operation)
                    .ok_or(CycleError::UnsupportedCycle)?,
                _ => return Err(CycleError::UnsupportedCycle),
            }
        };
        Ok(CycleRequest {
            cpu: self.clone(),
            bus: *bus,
            kind,
        })
    }

    pub fn retire_physical_cycle(
        &mut self,
        bus: &mut BusState,
        completion: CycleCompletion,
    ) -> Result<BoundedStep, CycleError> {
        self.retire_physical_cycle_capturing(bus, completion, &retirement_evidence::NoCapture)
    }

    pub fn retire_physical_cycle_with_evidence(
        &mut self,
        bus: &mut BusState,
        completion: CycleCompletion,
    ) -> Result<(BoundedStep, RetirementEvidence), CycleError> {
        let before = self.cycles;
        let kind = if matches!(completion.request.kind, CycleKind::M1 { halted: true, .. }) {
            RetirementKind::HaltPoll
        } else {
            RetirementKind::Instruction
        };
        let capture = retirement_evidence::InputCapture::default();
        let step = self.retire_physical_cycle_capturing(bus, completion, &capture)?;
        let mut evidence =
            RetirementEvidence::new(step.complete, capture.get(), before, self.cycles, kind);
        evidence.input_overflowed = capture.overflowed();
        Ok((step, evidence))
    }

    fn retire_physical_cycle_capturing<C: retirement_evidence::Capture>(
        &mut self,
        bus: &mut BusState,
        completion: CycleCompletion,
        capture: &C,
    ) -> Result<BoundedStep, CycleError> {
        let CycleCompletion {
            request,
            sample,
            tw,
            interrupts,
            base_t_states,
        } = completion;
        if request.cpu != *self || request.bus != *bus {
            return Err(CycleError::StaleContinuation);
        }
        request.validate_interrupts(interrupts)?;
        let kind = request.kind;
        if request.sampled_t_states(sample)? != base_t_states {
            return Err(CycleError::InvalidCompletion);
        }
        let cycles = base_t_states
            .checked_add(tw)
            .ok_or(CycleError::ClockOverflow)?;
        let total = self
            .cycles
            .checked_add(u64::from(cycles))
            .ok_or(CycleError::ClockOverflow)?;
        let mut cpu = self.clone();
        let mut candidate = *bus;
        let mut step = cpu.complete_physical_cycle(
            &mut candidate,
            kind,
            sample.value,
            base_t_states,
            capture,
        )?;
        if step.instruction.cycles != base_t_states
            || !cpu.extend_bus_segment(&mut step.instruction, tw)
            || cpu.cycles != total
        {
            return Err(CycleError::InvalidCompletion);
        }
        *self = cpu;
        *bus = candidate;
        Ok(step)
    }

    fn complete_physical_cycle<C: retirement_evidence::Capture>(
        &mut self,
        bus: &mut BusState,
        kind: CycleKind,
        value: u8,
        base_t_states: u32,
        capture: &C,
    ) -> Result<BoundedStep, CycleError> {
        match kind {
            CycleKind::M1 { halted: true, .. } => {
                self.clear_step_observation();
                Ok(BoundedStep {
                    instruction: self.complete_halt_fetch(),
                    complete: true,
                })
            }
            CycleKind::M1 {
                address,
                halted: false,
                ..
            } => {
                if bus.prefix.pending() {
                    return self.complete_index_fetch(bus, value, base_t_states, capture);
                }
                if bus.transfer.is_some() {
                    return self.complete_prefix_fetch(bus, value, capture);
                }
                self.clear_step_observation();
                self.accept_instruction_input(value, InstructionByteClass::M1, capture);
                self.increment_refresh_register();
                self.last_opcode_pc = address;
                self.last_opcode = value;
                if stack::Stack::prepares(value) {
                    return self.complete_stack_m1(bus, address, value, None, base_t_states);
                }
                let complete = if matches!(value, Z80_PREFIX_DD | Z80_PREFIX_FD) {
                    bus.prefix = PrefixState::index(address, value);
                    self.cycles = self.cycles.wrapping_add(u64::from(CYCLES_M1));
                    false
                } else if let Some(cycles) = self.execute_m1_register(value, None) {
                    self.finish_instruction(cycles);
                    true
                } else if let Some(cycles) = self.execute_register_cycle(value) {
                    self.finish_instruction(cycles);
                    true
                } else {
                    match value {
                        0x76 => {
                            let cycles = self.enter_halt();
                            self.finish_instruction(cycles);
                            true
                        }
                        _ => {
                            let operation = if value == Z80_PREFIX_CB {
                                Operation::CbFetch
                            } else if value == Z80_PREFIX_ED {
                                Operation::EdFetch { pc: address }
                            } else if let Some(load) = load::Load::decode(self, value, None) {
                                Operation::Load(load)
                            } else if operand_cycle::Operand::decode(self, value).is_some() {
                                Operation::Single {
                                    opcode: value,
                                    index: None,
                                }
                            } else if stack::Stack::is_jump(value)
                                || stack::Stack::is_pop(value)
                                || stack::Stack::is_call_exchange(value)
                            {
                                Operation::Stack(
                                    stack::Stack::decode(self, value, None)
                                        .ok_or(CycleError::UnsupportedCycle)?,
                                )
                            } else if let Some(operation) = rmw::Rmw::inc_dec(self, value) {
                                Operation::Rmw(operation)
                            } else {
                                return Err(CycleError::UnsupportedCycle);
                            };
                            bus.transfer = Some(Transfer {
                                pc: address,
                                operation,
                            });
                            self.cycles = self.cycles.wrapping_add(u64::from(CYCLES_M1));
                            false
                        }
                    }
                };
                Ok(BoundedStep {
                    instruction: FetchedInstruction {
                        pc: address,
                        opcode: value,
                        cycles: base_t_states,
                    },
                    complete,
                })
            }
            CycleKind::MemoryRead { .. }
            | CycleKind::MemoryReadExtended { .. }
            | CycleKind::MemoryWrite { .. }
            | CycleKind::MemoryWriteExtended { .. } => {
                let transfer = bus.transfer.ok_or(CycleError::InvalidCompletion)?;
                let sample = if kind.is_write() {
                    load::LoadSample::Written
                } else {
                    load::LoadSample::Read(value)
                };
                let next = match transfer.operation {
                    Operation::Load(load) => load
                        .complete_cycle(self, sample, capture)
                        .map_err(|()| CycleError::InvalidCompletion)?
                        .map(Operation::Load),
                    Operation::Single {
                        opcode,
                        index: None,
                    } if opcode == self.last_opcode => {
                        let operand = operand_cycle::Operand::decode(self, opcode)
                            .ok_or(CycleError::InvalidCompletion)?;
                        operand
                            .complete(self, sample, capture)
                            .map_err(|()| CycleError::InvalidCompletion)?;
                        None
                    }
                    Operation::Single {
                        opcode,
                        index: Some(prefix),
                    } => {
                        crate::index_cycle::Single::decode(self, opcode, prefix)
                            .ok_or(CycleError::InvalidCompletion)?
                            .complete(self, sample, capture)
                            .map_err(|()| CycleError::InvalidCompletion)?;
                        None
                    }
                    Operation::Stack(operation) => {
                        let next = if operation.is_call_exchange_phase() {
                            let (next, cycles) = operation
                                .complete_call_exchange(self, sample, true, capture)
                                .map_err(|()| CycleError::InvalidCompletion)?;
                            if cycles != base_t_states {
                                return Err(CycleError::InvalidCompletion);
                            }
                            Ok(next)
                        } else if kind.is_write() {
                            operation.complete_push(self)
                        } else if operation.is_jump_phase() {
                            operation.complete_jump(self, value, capture)
                        } else {
                            operation.complete_pop(self, value)
                        };
                        next.map_err(|()| CycleError::InvalidCompletion)?
                            .map(Operation::Stack)
                    }
                    Operation::Rmw(operation) => {
                        let sample = if kind.is_write() {
                            rmw::Sample::Written
                        } else {
                            rmw::Sample::Read(value)
                        };
                        let (next, cycles) = operation
                            .complete_cycle(self, sample)
                            .map_err(|()| CycleError::InvalidCompletion)?;
                        if cycles != base_t_states {
                            return Err(CycleError::InvalidCompletion);
                        }
                        next.map(Operation::Rmw)
                    }
                    _ => return Err(CycleError::InvalidCompletion),
                };
                bus.transfer = next.map(|operation| Transfer {
                    pc: transfer.pc,
                    operation,
                });
                if next.is_none() {
                    bus.prefix = PrefixState::default();
                    self.finish_instruction(base_t_states);
                } else {
                    self.cycles = self.cycles.wrapping_add(u64::from(base_t_states));
                }
                Ok(BoundedStep {
                    instruction: FetchedInstruction {
                        pc: transfer.pc,
                        opcode: self.last_opcode,
                        cycles: base_t_states,
                    },
                    complete: next.is_none(),
                })
            }
        }
    }
}

use super::*;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Service {
    pc: u16,
    kind: Kind,
    phase: Phase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Nmi,
    Maskable(InterruptMode, u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Acknowledge,
    Load(load::Load),
    Stack {
        operation: stack::Stack,
        vector: Option<u16>,
    },
    VectorLow(u16),
    VectorHigh(u16, u8),
}

impl Service {
    fn start(self, cpu: &mut Cpu) {
        cpu.state = CpuState::Running;
        cpu.interrupt_flip_flop_1 = false;
        cpu.enable_interrupts_delay = 0;
        cpu.last_step_was_interrupt = true;
        cpu.last_opcode_pc = self.pc;
        cpu.last_opcode = Z80_INTERRUPT_ACK_OPCODE;
        cpu.increment_refresh_register();
    }

    pub(super) fn acknowledge_im1(cpu: &mut Cpu) -> Self {
        let mut service = Self {
            pc: cpu.regs.pc,
            kind: Kind::Maskable(InterruptMode::Im1, cpu.regs.i),
            phase: Phase::Acknowledge,
        };
        service.start(cpu);
        cpu.interrupt_flip_flop_2 = false;
        service.phase = Phase::Stack {
            operation: stack::Stack::interrupt(cpu, Some(Z80_INTERRUPT_VECTOR_IM1)),
            vector: None,
        };
        service
    }

    pub(super) fn im1_write(self, cpu: &Cpu) -> Option<(u16, u8, bool)> {
        if self.kind != Kind::Maskable(InterruptMode::Im1, cpu.regs.i)
            || self.pc != cpu.regs.pc
            || cpu.state != CpuState::Running
            || cpu.interrupt_flip_flop_1
            || cpu.interrupt_flip_flop_2
            || cpu.enable_interrupts_delay != 0
            || !cpu.last_step_was_interrupt
            || cpu.last_opcode_pc != self.pc
            || cpu.last_opcode != Z80_INTERRUPT_ACK_OPCODE
            || cpu.instruction_byte_count != 0
            || cpu.instruction_bytes != [0; 4]
            || cpu.last_m1_fetch_count != 1
        {
            return None;
        }
        match self.phase {
            Phase::Stack {
                operation,
                vector: None,
            } => operation.im1_write(cpu),
            _ => None,
        }
    }

    pub(super) fn complete_im1_write(mut self, cpu: &mut Cpu) -> Result<Option<Self>, ()> {
        self.im1_write(cpu).ok_or(())?;
        let Phase::Stack {
            operation,
            vector: None,
        } = self.phase
        else {
            return Err(());
        };
        match operation.complete_push(cpu)? {
            Some(operation) => {
                self.phase = Phase::Stack {
                    operation,
                    vector: None,
                };
                Ok(Some(self))
            }
            None => Ok(None),
        }
    }

    pub(super) fn poll<B: Z80Bus>(cpu: &Cpu, bus: &B) -> Option<Self> {
        let kind = if bus.non_maskable_interrupt_pending() {
            Kind::Nmi
        } else if cpu.interrupt_flip_flop_1
            && cpu.enable_interrupts_delay == 0
            && bus.maskable_interrupt_pending()
        {
            Kind::Maskable(cpu.interrupt_mode, cpu.regs.i)
        } else {
            return None;
        };
        Some(Self {
            pc: cpu.regs.pc,
            kind,
            phase: Phase::Acknowledge,
        })
    }

    pub(super) fn pc(self) -> u16 {
        self.pc
    }

    pub(super) fn step<B: Z80Bus>(mut self, cpu: &mut Cpu, bus: &mut B) -> (Option<Self>, u32) {
        let cycles;
        self.phase = match self.phase {
            Phase::Acknowledge => {
                self.start(cpu);
                let (target, vector) = match self.kind {
                    Kind::Nmi => {
                        bus.acknowledge_non_maskable_interrupt();
                        bus.cpu_read(self.pc);
                        cycles = CYCLES_NMI_FETCH;
                        (Some(Z80_INTERRUPT_VECTOR_NMI), None)
                    }
                    Kind::Maskable(mode, i) => {
                        cpu.interrupt_flip_flop_2 = false;
                        let opcode = bus.acknowledge_maskable_interrupt();
                        if mode == InterruptMode::Im0 && opcode & 0xc7 != 0xc7 {
                            if opcode == 0 {
                                return (None, CYCLES_INTERRUPT_FETCH);
                            }
                            if let Some(operation) = load::Load::decode_interrupt(cpu, opcode) {
                                self.phase = Phase::Load(operation);
                                return (Some(self), CYCLES_INTERRUPT_FETCH);
                            }
                            if let Some(operation) = stack::Stack::decode_interrupt(cpu, opcode) {
                                self.phase = Phase::Stack {
                                    operation,
                                    vector: None,
                                };
                                return (Some(self), CYCLES_INTERRUPT_FETCH);
                            }
                            cpu.memptr = None;
                            cpu.trap = Some(CpuTrap::UnsupportedOpcode {
                                pc: self.pc,
                                opcode,
                            });
                            cpu.state = CpuState::Suspended;
                            return (None, CYCLES_INTERRUPT_FETCH);
                        }
                        cycles = CYCLES_INTERRUPT_FETCH + CYCLES_STACK_INTERNAL;
                        match mode {
                            InterruptMode::Im0 => (Some(u16::from(opcode & 0x38)), None),
                            InterruptMode::Im1 => (Some(Z80_INTERRUPT_VECTOR_IM1), None),
                            InterruptMode::Im2 => (None, Some(u16::from_be_bytes([i, opcode]))),
                        }
                    }
                };
                Phase::Stack {
                    operation: stack::Stack::interrupt(cpu, target),
                    vector,
                }
            }
            Phase::Load(operation) => {
                cycles = CYCLES_MEMORY_TRANSFER;
                match operation.step(cpu, bus) {
                    Some(operation) => Phase::Load(operation),
                    None => return (None, cycles),
                }
            }
            Phase::Stack { operation, vector } => {
                let (next, clocks) = operation.step(cpu, bus);
                cycles = clocks;
                match (next, vector) {
                    (Some(operation), _) => Phase::Stack { operation, vector },
                    (None, Some(address)) => Phase::VectorLow(address),
                    (None, None) => return (None, cycles),
                }
            }
            Phase::VectorLow(address) => {
                cycles = CYCLES_MEMORY_TRANSFER;
                Phase::VectorHigh(address.wrapping_add(1), bus.cpu_read(address))
            }
            Phase::VectorHigh(address, low) => {
                cpu.regs.pc = u16::from_le_bytes([low, bus.cpu_read(address)]);
                cpu.set_memptr(cpu.regs.pc);
                return (None, CYCLES_MEMORY_TRANSFER);
            }
        };
        (Some(self), cycles)
    }

    pub(super) fn execute<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> FetchedInstruction {
        let mut next = Some(self);
        let mut cycles = 0;
        while let Some(service) = next {
            let (pending, clocks) = service.step(cpu, bus);
            next = pending;
            cycles += clocks;
        }
        cpu.cycles = cpu.cycles.wrapping_add(u64::from(cycles));
        FetchedInstruction {
            pc: self.pc,
            opcode: Z80_INTERRUPT_ACK_OPCODE,
            cycles,
        }
    }
}

impl Cpu {
    pub(super) fn finish_instruction(&mut self, cycles: u32) {
        self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        if self.enable_interrupts_delay > 0 {
            self.enable_interrupts_delay -= 1;
            if self.enable_interrupts_delay == 0 {
                self.interrupt_flip_flop_1 = true;
                self.interrupt_flip_flop_2 = true;
            }
        }
    }

    pub(super) fn disable_interrupts(&mut self) {
        self.interrupt_flip_flop_1 = false;
        self.interrupt_flip_flop_2 = false;
        self.enable_interrupts_delay = 0;
    }

    pub(super) fn schedule_enable_interrupts(&mut self) {
        self.enable_interrupts_delay = 2;
    }
}

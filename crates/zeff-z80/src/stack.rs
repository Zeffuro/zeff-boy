use super::*;

mod call_exchange;
mod cycle;
mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stack {
    pair: Pair,
    address: u16,
    target: Option<u16>,
    phase: Phase,
    fetch: access::Fetch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pair {
    Register(u8),
    Index(bool),
    ProgramCounter,
    InterruptReturn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    JumpLow(Option<u8>),
    JumpHigh(Option<u8>, u8),
    CallLow(bool),
    CallHigh(bool, u8),
    ReturnPrepare(bool),
    PushPrepare(u16),
    PushHigh(u16),
    PushLow(u8),
    PopLow,
    PopHigh(u8),
    ExchangeLow(u16),
    ExchangeHigh(u16, u8),
    ExchangeWriteHigh(u16, u16),
    ExchangeWriteLow(u8, u16),
}

impl Pair {
    fn read(self, cpu: &Cpu) -> u16 {
        match self {
            Self::Register(pair) => cpu.read_stack_reg16(pair),
            Self::Index(false) => cpu.regs.ix,
            Self::Index(true) => cpu.regs.iy,
            Self::ProgramCounter | Self::InterruptReturn => cpu.regs.pc,
        }
    }

    fn write(self, cpu: &mut Cpu, value: u16) {
        match self {
            Self::Register(pair) => cpu.write_stack_reg16(pair, value),
            Self::Index(false) => cpu.regs.ix = value,
            Self::Index(true) => cpu.regs.iy = value,
            Self::ProgramCounter => {
                cpu.regs.pc = value;
                cpu.set_memptr(value);
            }
            Self::InterruptReturn => {
                cpu.regs.pc = value;
                cpu.set_memptr(value);
                cpu.interrupt_flip_flop_1 = cpu.interrupt_flip_flop_2;
            }
        }
    }
}

impl Stack {
    pub(super) fn is_jump_phase(&self) -> bool {
        matches!(self.phase, Phase::JumpLow(_) | Phase::JumpHigh(_, _))
    }

    pub(super) fn index_cycle(&self, cpu: &Cpu, opcode: u8, prefix: u8) -> Option<u16> {
        if !matches!(prefix, Z80_PREFIX_DD | Z80_PREFIX_FD)
            || self.fetch != access::Fetch::Memory
            || self.target.is_some()
            || self.address != cpu.regs.sp
        {
            return None;
        }
        let pair = if opcode == 0xc9 {
            Pair::ProgramCounter
        } else if ((opcode >> 4) & 3) == 2 {
            Pair::Index(prefix == Z80_PREFIX_FD)
        } else {
            Pair::Register((opcode >> 4) & 3)
        };
        if self.pair != pair {
            return None;
        }
        let valid = match self.phase {
            Phase::JumpLow(condition) if Self::is_jump(opcode) => {
                cpu.instruction_byte_count == 2
                    && condition == (opcode != 0xc3).then_some((opcode >> 3) & 7)
            }
            Phase::JumpHigh(condition, low) if Self::is_jump(opcode) => {
                cpu.instruction_byte_count == 3
                    && low == cpu.instruction_bytes[2]
                    && condition == (opcode != 0xc3).then_some((opcode >> 3) & 7)
            }
            Phase::PopLow | Phase::PopHigh(_) if Self::is_pop(opcode) => {
                cpu.instruction_byte_count == 2
            }
            _ => false,
        };
        valid.then_some(if Self::is_jump(opcode) {
            cpu.regs.pc
        } else {
            self.address
        })
    }

    pub(super) fn is_pop(opcode: u8) -> bool {
        matches!(opcode, 0xc1 | 0xd1 | 0xe1 | 0xf1 | 0xc9)
    }

    pub(super) fn is_ed_return(opcode: u8) -> bool {
        matches!(
            opcode,
            0x45 | 0x4d | 0x55 | 0x5d | 0x65 | 0x6d | 0x75 | 0x7d
        )
    }

    pub(super) fn pop_cycle(&self, cpu: &Cpu, opcode: u8, ed: bool) -> Option<u16> {
        let pair = if ed && Self::is_ed_return(opcode) {
            Pair::InterruptReturn
        } else if !ed && opcode == 0xc9 {
            Pair::ProgramCounter
        } else if !ed && Self::is_pop(opcode) {
            Pair::Register((opcode >> 4) & 3)
        } else {
            return None;
        };
        (self.pair == pair
            && self.fetch == access::Fetch::Memory
            && self.target.is_none()
            && self.address == cpu.regs.sp
            && matches!(self.phase, Phase::PopLow | Phase::PopHigh(_)))
        .then_some(self.address)
    }

    pub(super) fn complete_pop(mut self, cpu: &mut Cpu, value: u8) -> Result<Option<Self>, ()> {
        match self.phase {
            Phase::PopLow => {
                self.address = self.address.wrapping_add(1);
                cpu.regs.sp = self.address;
                self.phase = Phase::PopHigh(value);
                Ok(Some(self))
            }
            Phase::PopHigh(low) => {
                cpu.regs.sp = self.address.wrapping_add(1);
                self.pair.write(cpu, u16::from_le_bytes([low, value]));
                Ok(None)
            }
            _ => Err(()),
        }
    }

    pub(super) fn is_jump(opcode: u8) -> bool {
        matches!(
            opcode,
            0xc3 | 0xc2 | 0xca | 0xd2 | 0xda | 0xe2 | 0xea | 0xf2 | 0xfa
        )
    }

    pub(super) fn jump_cycle(&self, cpu: &Cpu, opcode: u8) -> Option<u16> {
        if !Self::is_jump(opcode)
            || self.fetch != access::Fetch::Memory
            || self.pair != Pair::Register((opcode >> 4) & 3)
            || self.target.is_some()
        {
            return None;
        }
        let expected = (opcode != 0xc3).then_some((opcode >> 3) & 7);
        match self.phase {
            Phase::JumpLow(condition) | Phase::JumpHigh(condition, _) if condition == expected => {
                Some(cpu.regs.pc)
            }
            _ => None,
        }
    }

    pub(super) fn complete_jump<C: retirement_evidence::Capture>(
        mut self,
        cpu: &mut Cpu,
        value: u8,
        capture: &C,
    ) -> Result<Option<Self>, ()> {
        if !matches!(self.phase, Phase::JumpLow(_) | Phase::JumpHigh(_, _)) {
            return Err(());
        }
        if self.fetch == access::Fetch::Memory {
            cpu.accept_instruction_input(value, InstructionByteClass::Operand, capture);
        }
        match self.phase {
            Phase::JumpLow(condition) => self.phase = Phase::JumpHigh(condition, value),
            Phase::JumpHigh(condition, low) => {
                cpu.set_memptr(u16::from_le_bytes([low, value]));
                if condition.is_none_or(|condition| cpu.condition_is_true(condition)) {
                    cpu.regs.pc = u16::from_le_bytes([low, value]);
                }
                return Ok(None);
            }
            _ => return Err(()),
        }
        Ok(Some(self))
    }

    pub(super) fn memory_cycle(&self) -> Option<u32> {
        (self.fetch == access::Fetch::Memory
            && self.pair != Pair::InterruptReturn
            && matches!(
                self.phase,
                Phase::JumpLow(_)
                    | Phase::JumpHigh(_, _)
                    | Phase::CallLow(_)
                    | Phase::CallHigh(_, _)
                    | Phase::PushHigh(_)
                    | Phase::PushLow(_)
                    | Phase::PopLow
                    | Phase::PopHigh(_)
                    | Phase::ExchangeLow(_)
                    | Phase::ExchangeWriteHigh(_, _)
            ))
        .then_some(CYCLES_MEMORY_TRANSFER)
    }

    pub(super) fn interrupt(cpu: &mut Cpu, target: Option<u16>) -> Self {
        let address = cpu.regs.sp.wrapping_sub(1);
        cpu.regs.sp = address;
        Self {
            pair: Pair::ProgramCounter,
            address,
            target,
            phase: Phase::PushHigh(cpu.regs.pc),
            fetch: access::Fetch::Memory,
        }
    }

    pub(super) fn decode(cpu: &Cpu, opcode: u8, index: Option<u8>) -> Option<Self> {
        let register = (opcode >> 4) & 3;
        let mut pair = match (register, index) {
            (2, Some(prefix)) => Pair::Index(prefix == Z80_PREFIX_FD),
            _ => Pair::Register(register),
        };
        let mut target = None;
        let phase = match opcode {
            0xc3 => Phase::JumpLow(None),
            0xc2 | 0xca | 0xd2 | 0xda | 0xe2 | 0xea | 0xf2 | 0xfa => {
                Phase::JumpLow(Some((opcode >> 3) & 7))
            }
            0xc5 | 0xd5 | 0xe5 | 0xf5 => Phase::PushPrepare(pair.read(cpu)),
            0xc1 | 0xd1 | 0xe1 | 0xf1 => Phase::PopLow,
            0xe3 => Phase::ExchangeLow(pair.read(cpu)),
            0xcd => Phase::CallLow(true),
            0xc4 | 0xcc | 0xd4 | 0xdc | 0xe4 | 0xec | 0xf4 | 0xfc => {
                Phase::CallLow(cpu.condition_is_true((opcode >> 3) & 7))
            }
            0xc9 => {
                pair = Pair::ProgramCounter;
                Phase::PopLow
            }
            0xc0 | 0xc8 | 0xd0 | 0xd8 | 0xe0 | 0xe8 | 0xf0 | 0xf8 => {
                pair = Pair::ProgramCounter;
                Phase::ReturnPrepare(cpu.condition_is_true((opcode >> 3) & 7))
            }
            0xc7 | 0xcf | 0xd7 | 0xdf | 0xe7 | 0xef | 0xf7 | 0xff => {
                target = Some(u16::from(opcode & 0x38));
                Phase::PushPrepare(cpu.regs.pc)
            }
            _ => return None,
        };
        Some(Self {
            pair,
            address: cpu.regs.sp,
            target,
            phase,
            fetch: access::Fetch::Memory,
        })
    }

    pub(super) fn decode_interrupt(cpu: &Cpu, opcode: u8) -> Option<Self> {
        if !matches!(
            opcode,
            0xc3 | 0xc2
                | 0xca
                | 0xd2
                | 0xda
                | 0xe2
                | 0xea
                | 0xf2
                | 0xfa
                | 0xcd
                | 0xc4
                | 0xcc
                | 0xd4
                | 0xdc
                | 0xe4
                | 0xec
                | 0xf4
                | 0xfc
        ) {
            return None;
        }
        let mut operation = Self::decode(cpu, opcode, None)?;
        operation.fetch = access::Fetch::Interrupt;
        Some(operation)
    }

    pub(super) fn decode_ed(cpu: &Cpu, opcode: u8) -> Option<Self> {
        match opcode {
            0x45 | 0x4d | 0x55 | 0x5d | 0x65 | 0x6d | 0x75 | 0x7d => Some(Self {
                pair: Pair::InterruptReturn,
                address: cpu.regs.sp,
                target: None,
                phase: Phase::PopLow,
                fetch: access::Fetch::Memory,
            }),
            _ => None,
        }
    }

    pub(super) fn step<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> (Option<Self>, u32) {
        if matches!(self.phase, Phase::ReturnPrepare(_) | Phase::PushPrepare(_)) {
            return (
                self.prepare(cpu).expect("matching stack preparation"),
                CYCLES_STACK_INTERNAL,
            );
        }
        if let Some((address, value)) = self.write_cycle() {
            bus.cpu_write(address, value);
            return (
                self.complete_push(cpu).expect("matching stack write"),
                CYCLES_MEMORY_TRANSFER,
            );
        }
        if matches!(self.phase, Phase::PopLow | Phase::PopHigh(_)) {
            let value = bus.cpu_read(self.address);
            return (
                self.complete_pop(cpu, value).expect("matching pop phase"),
                CYCLES_MEMORY_TRANSFER,
            );
        }
        if matches!(self.phase, Phase::JumpLow(_) | Phase::JumpHigh(_, _)) {
            let value = match self.fetch {
                access::Fetch::Memory => bus.cpu_read_instruction(cpu.regs.pc),
                access::Fetch::Interrupt => bus.read_interrupt_instruction(cpu.regs.pc),
            };
            return (
                self.complete_jump(cpu, value, &retirement_evidence::NoCapture)
                    .expect("matching jump phase"),
                CYCLES_MEMORY_TRANSFER,
            );
        }
        let sample = match self.phase {
            Phase::CallLow(_) | Phase::CallHigh(_, _) => load::LoadSample::Read(match self.fetch {
                access::Fetch::Memory => bus.cpu_read_instruction(cpu.regs.pc),
                access::Fetch::Interrupt => bus.read_interrupt_instruction(cpu.regs.pc),
            }),
            Phase::ExchangeLow(_) => load::LoadSample::Read(bus.cpu_read(self.address)),
            Phase::ExchangeHigh(_, _) => {
                load::LoadSample::Read(bus.cpu_read(self.address.wrapping_add(1)))
            }
            Phase::ExchangeWriteHigh(word, _) => {
                bus.cpu_write(self.address.wrapping_add(1), word.to_le_bytes()[1]);
                load::LoadSample::Written
            }
            Phase::ExchangeWriteLow(low, _) => {
                bus.cpu_write(self.address, low);
                load::LoadSample::Written
            }
            _ => unreachable!("stack phase handled above"),
        };
        // Publish architectural phases; EX's temporary electrical SP change is not exposed.
        self.complete_call_exchange(cpu, sample, false, &retirement_evidence::NoCapture)
            .expect("matching call or exchange phase")
    }

    pub(super) fn execute<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> u32 {
        let mut pending = Some(self);
        let mut cycles = CYCLES_M1;
        while let Some(operation) = pending {
            let (next, elapsed) = operation.step(cpu, bus);
            pending = next;
            cycles += elapsed;
        }
        cycles
    }
}

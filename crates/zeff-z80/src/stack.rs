use super::*;

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
            Self::ProgramCounter => cpu.regs.pc = value,
            Self::InterruptReturn => {
                cpu.regs.pc = value;
                cpu.interrupt_flip_flop_1 = cpu.interrupt_flip_flop_2;
            }
        }
    }
}

impl Stack {
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

    pub(super) fn step<B: Z80Bus>(mut self, cpu: &mut Cpu, bus: &mut B) -> (Option<Self>, u32) {
        let mut cycles = CYCLES_MEMORY_TRANSFER;
        self.phase = match self.phase {
            Phase::JumpLow(condition) => Phase::JumpHigh(condition, self.fetch.read(cpu, bus)),
            Phase::JumpHigh(condition, low) => {
                let high = self.fetch.read(cpu, bus);
                if condition.is_none_or(|condition| cpu.condition_is_true(condition)) {
                    cpu.regs.pc = u16::from_le_bytes([low, high]);
                }
                return (None, cycles);
            }
            Phase::CallLow(taken) => Phase::CallHigh(taken, self.fetch.read(cpu, bus)),
            Phase::CallHigh(taken, low) => {
                let high = self.fetch.read(cpu, bus);
                if !taken {
                    return (None, cycles);
                }
                self.target = Some(u16::from_le_bytes([low, high]));
                Phase::PushPrepare(cpu.regs.pc)
            }
            Phase::ReturnPrepare(taken) => {
                cycles = CYCLES_STACK_INTERNAL;
                if !taken {
                    return (None, cycles);
                }
                Phase::PopLow
            }
            Phase::PushPrepare(value) => {
                self.address = self.address.wrapping_sub(1);
                cpu.regs.sp = self.address;
                cycles = CYCLES_STACK_INTERNAL;
                Phase::PushHigh(value)
            }
            Phase::PushHigh(value) => {
                let [low, high] = value.to_le_bytes();
                bus.cpu_write(self.address, high);
                self.address = self.address.wrapping_sub(1);
                cpu.regs.sp = self.address;
                Phase::PushLow(low)
            }
            Phase::PushLow(low) => {
                bus.cpu_write(self.address, low);
                if let Some(target) = self.target {
                    cpu.regs.pc = target;
                }
                return (None, cycles);
            }
            Phase::PopLow => {
                let low = bus.cpu_read(self.address);
                self.address = self.address.wrapping_add(1);
                cpu.regs.sp = self.address;
                Phase::PopHigh(low)
            }
            Phase::PopHigh(low) => {
                let high = bus.cpu_read(self.address);
                cpu.regs.sp = self.address.wrapping_add(1);
                self.pair.write(cpu, u16::from_le_bytes([low, high]));
                return (None, cycles);
            }
            Phase::ExchangeLow(value) => Phase::ExchangeHigh(value, bus.cpu_read(self.address)),
            Phase::ExchangeHigh(value, low) => {
                let high = bus.cpu_read(self.address.wrapping_add(1));
                cycles = CYCLES_EXCHANGE_READ_HIGH;
                Phase::ExchangeWriteHigh(value, u16::from_le_bytes([low, high]))
            }
            Phase::ExchangeWriteHigh(value, result) => {
                let [low, high] = value.to_le_bytes();
                bus.cpu_write(self.address.wrapping_add(1), high);
                Phase::ExchangeWriteLow(low, result)
            }
            Phase::ExchangeWriteLow(low, result) => {
                bus.cpu_write(self.address, low);
                self.pair.write(cpu, result);
                return (None, CYCLES_EXCHANGE_WRITE_LOW);
            }
        };
        // Publish architectural phases; EX's temporary electrical SP change is not exposed.
        (Some(self), cycles)
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

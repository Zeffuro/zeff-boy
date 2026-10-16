use super::*;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Load {
    phase: Phase,
    fetch: access::Fetch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pair {
    Register(u8),
    Index(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Memory {
    ReadPair(Pair),
    WritePair(u16),
    ReadAccumulator,
    WriteAccumulator(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    ImmediateRegister(u8),
    ImmediateLow(Pair),
    ImmediateHigh(Pair, u8),
    AddressLow(Memory),
    AddressHigh(Memory, u8),
    Memory(Memory, u16),
    ReadHigh(Pair, u16, u8),
    WriteHigh(u16, u8),
    ImmediateByte(u16),
    WriteByte(u16, u8),
}

impl Pair {
    fn read(self, cpu: &Cpu) -> u16 {
        match self {
            Self::Register(pair) => cpu.read_reg16(pair),
            Self::Index(false) => cpu.regs.ix,
            Self::Index(true) => cpu.regs.iy,
        }
    }

    fn write(self, cpu: &mut Cpu, value: u16) {
        match self {
            Self::Register(pair) => cpu.write_reg16(pair, value),
            Self::Index(false) => cpu.regs.ix = value,
            Self::Index(true) => cpu.regs.iy = value,
        }
    }
}

impl Load {
    pub(super) fn memory_cycle(&self) -> Option<u32> {
        (self.fetch == access::Fetch::Memory).then_some(CYCLES_MEMORY_TRANSFER)
    }

    pub(super) fn decode(cpu: &Cpu, opcode: u8, index: Option<u8>) -> Option<Self> {
        let pair = |register| match (register, index) {
            (2, Some(prefix)) => Pair::Index(prefix == Z80_PREFIX_FD),
            _ => Pair::Register(register),
        };
        let phase = match opcode {
            0x06 | 0x0e | 0x16 | 0x1e | 0x26 | 0x2e | 0x3e if index.is_none() => {
                Phase::ImmediateRegister((opcode >> 3) & 7)
            }
            0x01 | 0x11 | 0x21 | 0x31 => Phase::ImmediateLow(pair((opcode >> 4) & 3)),
            0x22 => Phase::AddressLow(Memory::WritePair(pair(2).read(cpu))),
            0x2a => Phase::AddressLow(Memory::ReadPair(pair(2))),
            0x32 => Phase::AddressLow(Memory::WriteAccumulator(cpu.regs.a)),
            0x3a => Phase::AddressLow(Memory::ReadAccumulator),
            0x36 if index.is_none() => Phase::ImmediateByte(cpu.regs.hl()),
            _ => return None,
        };
        Some(Self {
            phase,
            fetch: access::Fetch::Memory,
        })
    }

    pub(super) fn decode_interrupt(cpu: &Cpu, opcode: u8) -> Option<Self> {
        if !matches!(
            opcode,
            0x01 | 0x11 | 0x21 | 0x31 | 0x06 | 0x0e | 0x16 | 0x1e | 0x26 | 0x2e | 0x3e
        ) {
            return None;
        }
        let mut operation = Self::decode(cpu, opcode, None)?;
        operation.fetch = access::Fetch::Interrupt;
        Some(operation)
    }

    pub(super) fn decode_ed(cpu: &Cpu, opcode: u8) -> Option<Self> {
        let pair = Pair::Register((opcode >> 4) & 3);
        let memory = match opcode {
            0x43 | 0x53 | 0x63 | 0x73 => Memory::WritePair(pair.read(cpu)),
            0x4b | 0x5b | 0x6b | 0x7b => Memory::ReadPair(pair),
            _ => return None,
        };
        Some(Self {
            phase: Phase::AddressLow(memory),
            fetch: access::Fetch::Memory,
        })
    }

    pub(super) fn step<B: Z80Bus>(mut self, cpu: &mut Cpu, bus: &mut B) -> Option<Self> {
        self.phase = match self.phase {
            Phase::ImmediateRegister(register) => {
                let value = self.fetch.read(cpu, bus);
                cpu.write_reg8(bus, register, value);
                return None;
            }
            Phase::ImmediateLow(pair) => Phase::ImmediateHigh(pair, self.fetch.read(cpu, bus)),
            Phase::ImmediateHigh(pair, low) => {
                let high = self.fetch.read(cpu, bus);
                pair.write(cpu, u16::from_le_bytes([low, high]));
                return None;
            }
            Phase::AddressLow(memory) => Phase::AddressHigh(memory, self.fetch.read(cpu, bus)),
            Phase::AddressHigh(memory, low) => {
                let high = self.fetch.read(cpu, bus);
                Phase::Memory(memory, u16::from_le_bytes([low, high]))
            }
            Phase::Memory(memory, address) => match memory {
                Memory::ReadPair(pair) => {
                    Phase::ReadHigh(pair, address.wrapping_add(1), bus.cpu_read(address))
                }
                Memory::WritePair(value) => {
                    let [low, high] = value.to_le_bytes();
                    bus.cpu_write(address, low);
                    Phase::WriteHigh(address.wrapping_add(1), high)
                }
                Memory::ReadAccumulator => {
                    cpu.regs.a = bus.cpu_read(address);
                    return None;
                }
                Memory::WriteAccumulator(value) => {
                    bus.cpu_write(address, value);
                    return None;
                }
            },
            Phase::ReadHigh(pair, address, low) => {
                let high = bus.cpu_read(address);
                pair.write(cpu, u16::from_le_bytes([low, high]));
                return None;
            }
            Phase::WriteHigh(address, high) => {
                bus.cpu_write(address, high);
                return None;
            }
            Phase::ImmediateByte(address) => Phase::WriteByte(address, self.fetch.read(cpu, bus)),
            Phase::WriteByte(address, value) => {
                bus.cpu_write(address, value);
                return None;
            }
        };
        Some(self)
    }

    pub(super) fn execute<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> u32 {
        let mut pending = Some(self);
        let mut cycles = CYCLES_M1;
        while let Some(load) = pending {
            pending = load.step(cpu, bus);
            cycles += CYCLES_MEMORY_TRANSFER;
        }
        cycles
    }
}

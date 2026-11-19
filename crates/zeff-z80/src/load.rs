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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LoadCycle {
    Fetch { address: u16 },
    Read { address: u16 },
    Write { address: u16, value: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LoadSample {
    Read(u8),
    Written,
}

impl LoadSample {
    fn read(self) -> Result<u8, ()> {
        match self {
            Self::Read(value) => Ok(value),
            Self::Written => Err(()),
        }
    }
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
    fn cycle(&self, cpu: &Cpu) -> LoadCycle {
        match self.phase {
            Phase::ImmediateRegister(_)
            | Phase::ImmediateLow(_)
            | Phase::ImmediateHigh(_, _)
            | Phase::AddressLow(_)
            | Phase::AddressHigh(_, _)
            | Phase::ImmediateByte(_) => LoadCycle::Fetch {
                address: cpu.regs.pc,
            },
            Phase::Memory(Memory::ReadPair(_) | Memory::ReadAccumulator, address)
            | Phase::ReadHigh(_, address, _) => LoadCycle::Read { address },
            Phase::Memory(Memory::WritePair(value), address) => LoadCycle::Write {
                address,
                value: value.to_le_bytes()[0],
            },
            Phase::Memory(Memory::WriteAccumulator(value), address)
            | Phase::WriteHigh(address, value)
            | Phase::WriteByte(address, value) => LoadCycle::Write { address, value },
        }
    }

    pub(super) fn physical_cycle(&self, cpu: &Cpu, opcode: u8) -> Option<LoadCycle> {
        if self.fetch != access::Fetch::Memory {
            return None;
        }
        let valid = match (opcode, self.phase) {
            (
                0x06 | 0x0e | 0x16 | 0x1e | 0x26 | 0x2e | 0x3e,
                Phase::ImmediateRegister(register),
            ) => register == ((opcode >> 3) & 7),
            (
                0x01 | 0x11 | 0x21 | 0x31,
                Phase::ImmediateLow(Pair::Register(pair))
                | Phase::ImmediateHigh(Pair::Register(pair), _),
            ) => pair == ((opcode >> 4) & 3),
            (
                0x22,
                Phase::AddressLow(Memory::WritePair(_))
                | Phase::AddressHigh(Memory::WritePair(_), _)
                | Phase::Memory(Memory::WritePair(_), _)
                | Phase::WriteHigh(_, _),
            ) => true,
            (
                0x2a,
                Phase::AddressLow(Memory::ReadPair(Pair::Register(2)))
                | Phase::AddressHigh(Memory::ReadPair(Pair::Register(2)), _)
                | Phase::Memory(Memory::ReadPair(Pair::Register(2)), _)
                | Phase::ReadHigh(Pair::Register(2), _, _),
            ) => true,
            (
                0x32,
                Phase::AddressLow(Memory::WriteAccumulator(_))
                | Phase::AddressHigh(Memory::WriteAccumulator(_), _)
                | Phase::Memory(Memory::WriteAccumulator(_), _),
            ) => true,
            (
                0x3a,
                Phase::AddressLow(Memory::ReadAccumulator)
                | Phase::AddressHigh(Memory::ReadAccumulator, _)
                | Phase::Memory(Memory::ReadAccumulator, _),
            ) => true,
            (0x36, Phase::ImmediateByte(_) | Phase::WriteByte(_, _)) => true,
            _ => false,
        };
        if valid { Some(self.cycle(cpu)) } else { None }
    }

    pub(super) fn physical_ed_cycle(&self, cpu: &Cpu, opcode: u8) -> Option<LoadCycle> {
        if self.fetch != access::Fetch::Memory {
            return None;
        }
        let valid = match (opcode, self.phase) {
            (
                0x43 | 0x53 | 0x63 | 0x73,
                Phase::AddressLow(Memory::WritePair(_))
                | Phase::AddressHigh(Memory::WritePair(_), _)
                | Phase::Memory(Memory::WritePair(_), _)
                | Phase::WriteHigh(_, _),
            ) => true,
            (
                0x4b | 0x5b | 0x6b | 0x7b,
                Phase::AddressLow(Memory::ReadPair(Pair::Register(pair)))
                | Phase::AddressHigh(Memory::ReadPair(Pair::Register(pair)), _)
                | Phase::Memory(Memory::ReadPair(Pair::Register(pair)), _)
                | Phase::ReadHigh(Pair::Register(pair), _, _),
            ) => pair == ((opcode >> 4) & 3),
            _ => false,
        };
        let bytes = match self.phase {
            Phase::AddressLow(_) => 2,
            Phase::AddressHigh(_, _) => 3,
            Phase::Memory(_, _) | Phase::ReadHigh(_, _, _) | Phase::WriteHigh(_, _) => 4,
            _ => return None,
        };
        if !valid || cpu.instruction_byte_count != bytes {
            return None;
        }
        let word = cpu.read_reg16((opcode >> 4) & 3);
        let decoded_address =
            u16::from_le_bytes([cpu.instruction_bytes[2], cpu.instruction_bytes[3]]);
        let captured_word = match self.phase {
            Phase::AddressLow(Memory::WritePair(value))
            | Phase::AddressHigh(Memory::WritePair(value), _)
            | Phase::Memory(Memory::WritePair(value), _) => Some(value),
            _ => None,
        };
        if captured_word.is_some_and(|value| value != word) {
            return None;
        }
        let valid_snapshot = match self.phase {
            Phase::AddressLow(_) => true,
            Phase::AddressHigh(_, low) => low == cpu.instruction_bytes[2],
            Phase::Memory(_, address) => address == decoded_address,
            Phase::ReadHigh(_, address, _) => address == decoded_address.wrapping_add(1),
            Phase::WriteHigh(address, high) => {
                address == decoded_address.wrapping_add(1) && high == word.to_le_bytes()[1]
            }
            _ => false,
        };
        valid_snapshot.then(|| self.cycle(cpu))
    }

    pub(super) fn physical_index_cycle(
        &self,
        cpu: &Cpu,
        opcode: u8,
        prefix: u8,
    ) -> Option<LoadCycle> {
        if self.fetch != access::Fetch::Memory || !matches!(prefix, Z80_PREFIX_DD | Z80_PREFIX_FD) {
            return None;
        }
        let selected = Pair::Index(prefix == Z80_PREFIX_FD);
        let valid = match (opcode, self.phase) {
            (
                0x01 | 0x11 | 0x21 | 0x31,
                Phase::ImmediateLow(pair) | Phase::ImmediateHigh(pair, _),
            ) => {
                pair == if opcode == 0x21 {
                    selected
                } else {
                    Pair::Register((opcode >> 4) & 3)
                }
            }
            (
                0x22,
                Phase::AddressLow(Memory::WritePair(_))
                | Phase::AddressHigh(Memory::WritePair(_), _)
                | Phase::Memory(Memory::WritePair(_), _)
                | Phase::WriteHigh(_, _),
            ) => true,
            (
                0x2a,
                Phase::AddressLow(Memory::ReadPair(pair))
                | Phase::AddressHigh(Memory::ReadPair(pair), _)
                | Phase::Memory(Memory::ReadPair(pair), _)
                | Phase::ReadHigh(pair, _, _),
            ) => pair == selected,
            (
                0x32,
                Phase::AddressLow(Memory::WriteAccumulator(_))
                | Phase::AddressHigh(Memory::WriteAccumulator(_), _)
                | Phase::Memory(Memory::WriteAccumulator(_), _),
            ) => true,
            (
                0x3a,
                Phase::AddressLow(Memory::ReadAccumulator)
                | Phase::AddressHigh(Memory::ReadAccumulator, _)
                | Phase::Memory(Memory::ReadAccumulator, _),
            ) => true,
            _ => false,
        };
        let bytes = match self.phase {
            Phase::ImmediateLow(_) | Phase::AddressLow(_) => 2,
            Phase::ImmediateHigh(_, _) | Phase::AddressHigh(_, _) => 3,
            Phase::Memory(_, _) | Phase::ReadHigh(_, _, _) | Phase::WriteHigh(_, _) => 4,
            _ => return None,
        };
        if !valid || cpu.instruction_byte_count != bytes {
            return None;
        }
        let selected_word = selected.read(cpu);
        let captured_word = match self.phase {
            Phase::AddressLow(Memory::WritePair(value))
            | Phase::AddressHigh(Memory::WritePair(value), _)
            | Phase::Memory(Memory::WritePair(value), _) => Some(value),
            _ => None,
        };
        if captured_word.is_some_and(|value| value != selected_word) {
            return None;
        }
        let captured_a = match self.phase {
            Phase::AddressLow(Memory::WriteAccumulator(value))
            | Phase::AddressHigh(Memory::WriteAccumulator(value), _)
            | Phase::Memory(Memory::WriteAccumulator(value), _) => Some(value),
            _ => None,
        };
        if captured_a.is_some_and(|value| value != cpu.regs.a) {
            return None;
        }
        let address = u16::from_le_bytes([cpu.instruction_bytes[2], cpu.instruction_bytes[3]]);
        let valid_capture = match self.phase {
            Phase::ImmediateLow(_) | Phase::AddressLow(_) => true,
            Phase::ImmediateHigh(_, low) | Phase::AddressHigh(_, low) => {
                low == cpu.instruction_bytes[2]
            }
            Phase::Memory(_, captured) => captured == address,
            Phase::ReadHigh(_, captured, _) => captured == address.wrapping_add(1),
            Phase::WriteHigh(captured, high) => {
                captured == address.wrapping_add(1) && high == selected_word.to_le_bytes()[1]
            }
            _ => false,
        };
        valid_capture.then(|| self.cycle(cpu))
    }

    pub(super) fn complete_cycle<C: retirement_evidence::Capture>(
        mut self,
        cpu: &mut Cpu,
        sample: LoadSample,
        capture: &C,
    ) -> Result<Option<Self>, ()> {
        match (self.cycle(cpu), sample) {
            (LoadCycle::Fetch { .. }, LoadSample::Read(value)) => {
                if self.fetch == access::Fetch::Memory {
                    cpu.accept_instruction_input(value, InstructionByteClass::Operand, capture);
                }
            }
            (LoadCycle::Read { .. }, LoadSample::Read(_))
            | (LoadCycle::Write { .. }, LoadSample::Written) => {}
            _ => return Err(()),
        }
        self.phase = match self.phase {
            Phase::ImmediateRegister(register) => {
                let destination = match register {
                    0 => &mut cpu.regs.b,
                    1 => &mut cpu.regs.c,
                    2 => &mut cpu.regs.d,
                    3 => &mut cpu.regs.e,
                    4 => &mut cpu.regs.h,
                    5 => &mut cpu.regs.l,
                    REGISTER_A_INDEX => &mut cpu.regs.a,
                    _ => return Err(()),
                };
                *destination = sample.read()?;
                return Ok(None);
            }
            Phase::ImmediateLow(pair) => Phase::ImmediateHigh(pair, sample.read()?),
            Phase::ImmediateHigh(pair, low) => {
                pair.write(cpu, u16::from_le_bytes([low, sample.read()?]));
                return Ok(None);
            }
            Phase::AddressLow(memory) => Phase::AddressHigh(memory, sample.read()?),
            Phase::AddressHigh(memory, low) => {
                Phase::Memory(memory, u16::from_le_bytes([low, sample.read()?]))
            }
            Phase::Memory(Memory::ReadPair(pair), address) => {
                Phase::ReadHigh(pair, address.wrapping_add(1), sample.read()?)
            }
            Phase::Memory(Memory::WritePair(value), address) => {
                Phase::WriteHigh(address.wrapping_add(1), value.to_le_bytes()[1])
            }
            Phase::Memory(Memory::ReadAccumulator, address) => {
                cpu.regs.a = sample.read()?;
                cpu.set_memptr(address.wrapping_add(1));
                return Ok(None);
            }
            Phase::ReadHigh(pair, address, low) => {
                pair.write(cpu, u16::from_le_bytes([low, sample.read()?]));
                cpu.set_memptr(address);
                return Ok(None);
            }
            Phase::ImmediateByte(address) => Phase::WriteByte(address, sample.read()?),
            Phase::Memory(Memory::WriteAccumulator(value), address) => {
                cpu.memptr_accumulator_store(address, value);
                return Ok(None);
            }
            Phase::WriteHigh(address, _) => {
                cpu.set_memptr(address);
                return Ok(None);
            }
            Phase::WriteByte(_, _) => return Ok(None),
        };
        Ok(Some(self))
    }

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

    pub(super) fn step<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> Option<Self> {
        let sample = match self.cycle(cpu) {
            LoadCycle::Fetch { address } => LoadSample::Read(match self.fetch {
                access::Fetch::Memory => bus.cpu_read_instruction(address),
                access::Fetch::Interrupt => bus.read_interrupt_instruction(address),
            }),
            LoadCycle::Read { address } => LoadSample::Read(bus.cpu_read(address)),
            LoadCycle::Write { address, value } => {
                bus.cpu_write(address, value);
                LoadSample::Written
            }
        };
        self.complete_cycle(cpu, sample, &retirement_evidence::NoCapture)
            .expect("matching load cycle sample")
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

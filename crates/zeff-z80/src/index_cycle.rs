use super::*;
use register_cycle::{Register, RegisterCycle};

#[derive(Clone, Copy)]
enum Index {
    Ix,
    Iy,
}

impl Index {
    fn decode(prefix: u8) -> Option<Self> {
        match prefix {
            Z80_PREFIX_DD => Some(Self::Ix),
            Z80_PREFIX_FD => Some(Self::Iy),
            _ => None,
        }
    }

    fn read(self, cpu: &Cpu) -> u16 {
        match self {
            Self::Ix => cpu.regs.ix,
            Self::Iy => cpu.regs.iy,
        }
    }

    fn write(self, cpu: &mut Cpu, value: u16) {
        match self {
            Self::Ix => cpu.regs.ix = value,
            Self::Iy => cpu.regs.iy = value,
        }
    }

    fn read_register(self, cpu: &Cpu, register: Register) -> u8 {
        match register {
            Register::H => self.read(cpu).to_be_bytes()[0],
            Register::L => self.read(cpu).to_be_bytes()[1],
            _ => register.read(cpu),
        }
    }

    fn write_register(self, cpu: &mut Cpu, register: Register, value: u8) {
        let [high, low] = self.read(cpu).to_be_bytes();
        match register {
            Register::H => self.write(cpu, u16::from_be_bytes([value, low])),
            Register::L => self.write(cpu, u16::from_be_bytes([high, value])),
            _ => register.write(cpu, value),
        }
    }
}

pub(super) fn admitted(opcode: u8) -> bool {
    m1_register::admitted(opcode)
        || stack::Stack::prepares(opcode)
        || stack::Stack::is_call_exchange(opcode)
        || register_cycle::decode(opcode).is_some()
        || matches!(
            opcode,
            0x76 | 0x06
                | 0x0e
                | 0x16
                | 0x1e
                | 0x26
                | 0x2e
                | 0x3e
                | 0x01
                | 0x11
                | 0x21
                | 0x31
                | 0x22
                | 0x2a
                | 0x32
                | 0x3a
                | 0x02
                | 0x12
                | 0x0a
                | 0x1a
                | 0xc6
                | 0xce
                | 0xd6
                | 0xde
                | 0xe6
                | 0xee
                | 0xf6
                | 0xfe
        )
        || stack::Stack::is_jump(opcode)
        || stack::Stack::is_pop(opcode)
}

#[derive(Clone, Copy)]
pub(super) enum Single {
    Immediate { prefix: u8, register: Register },
    Operand(operand_cycle::Operand),
}

impl Single {
    pub(super) fn decode(cpu: &Cpu, opcode: u8, prefix: u8) -> Option<Self> {
        Index::decode(prefix)?;
        match opcode {
            0x06 | 0x0e | 0x16 | 0x1e | 0x26 | 0x2e | 0x3e => Some(Self::Immediate {
                prefix,
                register: Register::decode((opcode >> 3) & 7)?,
            }),
            0x02 | 0x12 | 0x0a | 0x1a | 0xc6 | 0xce | 0xd6 | 0xde | 0xe6 | 0xee | 0xf6 | 0xfe => {
                Some(Self::Operand(operand_cycle::Operand::decode(cpu, opcode)?))
            }
            _ => None,
        }
    }

    pub(super) fn cycle(self, cpu: &Cpu, opcode: u8) -> Option<load::LoadCycle> {
        match self {
            Self::Immediate { .. } => Some(load::LoadCycle::Fetch {
                address: cpu.regs.pc,
            }),
            Self::Operand(operand) => operand.physical_cycle(cpu, opcode),
        }
    }

    pub(super) fn complete<C: retirement_evidence::Capture>(
        self,
        cpu: &mut Cpu,
        sample: load::LoadSample,
        capture: &C,
    ) -> Result<(), ()> {
        match self {
            Self::Immediate { prefix, register } => {
                let load::LoadSample::Read(value) = sample else {
                    return Err(());
                };
                cpu.accept_instruction_input(value, InstructionByteClass::Operand, capture);
                Index::decode(prefix)
                    .ok_or(())?
                    .write_register(cpu, register, value);
                Ok(())
            }
            Self::Operand(operand) => operand.complete(cpu, sample, capture),
        }
    }

    pub(super) fn step<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B, opcode: u8) {
        let sample = match self.cycle(cpu, opcode).expect("matching index single") {
            load::LoadCycle::Fetch { address } => {
                load::LoadSample::Read(bus.cpu_read_instruction(address))
            }
            load::LoadCycle::Read { address } => load::LoadSample::Read(bus.cpu_read(address)),
            load::LoadCycle::Write { address, value } => {
                bus.cpu_write(address, value);
                load::LoadSample::Written
            }
        };
        self.complete(cpu, sample, &retirement_evidence::NoCapture)
            .expect("matching index sample");
    }
}

impl Cpu {
    pub(super) fn execute_index_register(&mut self, prefix: u8, opcode: u8) -> Option<u32> {
        let index = Index::decode(prefix)?;
        match register_cycle::decode(opcode)? {
            RegisterCycle::Increment(register) => {
                let value = index.read_register(self, register);
                let result = value.wrapping_add(1);
                index.write_register(self, register, result);
                self.set_inc_flags(value, result);
            }
            RegisterCycle::Decrement(register) => {
                let value = index.read_register(self, register);
                let result = value.wrapping_sub(1);
                index.write_register(self, register, result);
                self.set_dec_flags(value, result);
            }
            RegisterCycle::Move {
                destination,
                source,
            } => {
                let value = index.read_register(self, source);
                index.write_register(self, destination, value);
            }
            RegisterCycle::Alu { group, source } => {
                let value = index.read_register(self, source);
                self.execute_alu_group(group, value);
            }
            RegisterCycle::JumpHl => self.regs.pc = index.read(self),
            _ => return self.execute_register_cycle(opcode),
        }
        Some(CYCLES_M1)
    }
}

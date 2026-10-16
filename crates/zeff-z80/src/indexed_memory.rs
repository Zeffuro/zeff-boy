use super::*;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct IndexedMemory(Phase);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Read(u8),
    Write(u8),
    Alu(u8),
    IncDec(u8),
    Immediate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Displacement(Operation, u16),
    Address(Operation, u16),
    Immediate(u16),
    Memory(Operation, u16),
    Rmw(rmw::Rmw),
}

impl IndexedMemory {
    pub(super) fn decode(cpu: &Cpu, opcode: u8, prefix: u8) -> Option<Self> {
        let operation = match opcode {
            0x46 | 0x4e | 0x56 | 0x5e | 0x66 | 0x6e | 0x7e => Operation::Read((opcode >> 3) & 7),
            0x70..=0x75 | 0x77 => Operation::Write(cpu.read_register8(opcode & 7)),
            0x86 | 0x8e | 0x96 | 0x9e | 0xa6 | 0xae | 0xb6 | 0xbe => {
                Operation::Alu((opcode >> 3) & 7)
            }
            0x34 | 0x35 => Operation::IncDec(opcode),
            0x36 => Operation::Immediate,
            _ => return None,
        };
        let base = if prefix == Z80_PREFIX_FD {
            cpu.regs.iy
        } else {
            cpu.regs.ix
        };
        Some(Self(Phase::Displacement(operation, base)))
    }

    pub(super) fn step<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> (Option<Self>, u32) {
        let (next, cycles) = match self.0 {
            Phase::Displacement(operation, base) => {
                let displacement = cpu.fetch_u8(bus) as i8;
                let address = base.wrapping_add_signed(i16::from(displacement));
                let next = if operation == Operation::Immediate {
                    Phase::Immediate(address)
                } else {
                    Phase::Address(operation, address)
                };
                (next, CYCLES_MEMORY_TRANSFER)
            }
            Phase::Address(operation, address) => {
                let next = match operation {
                    Operation::IncDec(opcode) => {
                        Phase::Rmw(rmw::Rmw::inc_dec_at(address, opcode).unwrap())
                    }
                    _ => Phase::Memory(operation, address),
                };
                (next, CYCLES_INDEX_ADDRESS)
            }
            Phase::Immediate(address) => (
                Phase::Memory(Operation::Write(cpu.fetch_u8(bus)), address),
                CYCLES_INDEX_IMMEDIATE,
            ),
            Phase::Memory(operation, address) => {
                match operation {
                    Operation::Read(register) => {
                        let value = bus.cpu_read(address);
                        cpu.write_reg8(bus, register, value);
                    }
                    Operation::Write(value) => bus.cpu_write(address, value),
                    Operation::Alu(group) => cpu.execute_alu_group(group, bus.cpu_read(address)),
                    _ => unreachable!("indexed memory operation resolved before transfer"),
                }
                return (None, CYCLES_MEMORY_TRANSFER);
            }
            Phase::Rmw(rmw) => {
                let (next, cycles) = rmw.step(cpu, bus);
                return (next.map(|rmw| Self(Phase::Rmw(rmw))), cycles);
            }
        };
        (Some(Self(next)), cycles)
    }

    pub(super) fn execute<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> u32 {
        let mut pending = Some(self);
        let mut cycles = Z80_PREFIX_OVERHEAD + CYCLES_M1;
        while let Some(operation) = pending {
            let (next, elapsed) = operation.step(cpu, bus);
            pending = next;
            cycles += elapsed;
        }
        cycles
    }
}

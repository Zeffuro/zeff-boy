use super::*;
use crate::IndexedMemoryKind;

impl IndexedMemory {
    pub(crate) fn typed_pending(self) -> bool {
        match self.0 {
            Phase::Displacement(Operation::Immediate, _) => true,
            Phase::Displacement(op, _) | Phase::Address(op, _) | Phase::Memory(op, _) => {
                matches!(
                    op,
                    Operation::Read(_)
                        | Operation::Write(_)
                        | Operation::Alu(_)
                        | Operation::IncDec(_)
                )
            }
            Phase::Immediate(_) | Phase::Rmw(_) => true,
        }
    }

    pub(crate) fn typed_kind(
        self,
        cpu: &Cpu,
        opcode: u8,
        prefix: u8,
    ) -> Result<IndexedMemoryKind, CycleError> {
        let expected = match opcode {
            0x34 | 0x35 => Operation::IncDec(opcode),
            0x36 if matches!(self.0, Phase::Memory(..)) => {
                Operation::Write(cpu.instruction_bytes[3])
            }
            0x36 => Operation::Immediate,
            0x46 | 0x4e | 0x56 | 0x5e | 0x66 | 0x6e | 0x7e => Operation::Read((opcode >> 3) & 7),
            0x70..=0x75 | 0x77 => Operation::Write(cpu.read_register8(opcode & 7)),
            0x86 | 0x8e | 0x96 | 0x9e | 0xa6 | 0xae | 0xb6 | 0xbe => {
                Operation::Alu((opcode >> 3) & 7)
            }
            _ => return Err(CycleError::UnsupportedCycle),
        };
        let base = if prefix == Z80_PREFIX_FD {
            cpu.regs.iy
        } else {
            cpu.regs.ix
        };
        let expected_bytes = if matches!(self.0, Phase::Displacement(..)) {
            2
        } else if opcode == 0x36 && matches!(self.0, Phase::Memory(..)) {
            4
        } else {
            3
        };
        if cpu.instruction_byte_count != expected_bytes {
            return Err(CycleError::UnsupportedCycle);
        }
        let effective = base.wrapping_add_signed(i16::from(cpu.instruction_bytes[2] as i8));
        match self.0 {
            Phase::Displacement(op, captured) if op == expected && captured == base => {
                let previous_r = (cpu.regs.r & 0x80) | (cpu.regs.r.wrapping_sub(1) & 0x7f);
                Ok(IndexedMemoryKind::Displacement {
                    address: cpu.regs.pc,
                    preceding_refresh: u16::from_be_bytes([cpu.regs.i, previous_r]),
                })
            }
            Phase::Address(op, address)
                if op == expected && op != Operation::Immediate && address == effective =>
            {
                Ok(IndexedMemoryKind::Address {
                    retained_address: cpu.regs.pc.wrapping_sub(1),
                })
            }
            Phase::Immediate(address)
                if expected == Operation::Immediate && address == effective =>
            {
                Ok(IndexedMemoryKind::Immediate {
                    address: cpu.regs.pc,
                })
            }
            Phase::Memory(op, address) if op == expected && address == effective => Ok(match op {
                Operation::Read(_) | Operation::Alu(_) => IndexedMemoryKind::Read { address },
                Operation::Write(value) => IndexedMemoryKind::Write { address, value },
                _ => return Err(CycleError::UnsupportedCycle),
            }),
            Phase::Rmw(operation)
                if matches!(expected, Operation::IncDec(_))
                    && (cpu.memptr_profile == MemptrProfile::Legacy
                        || cpu.memptr == Some(effective)) =>
            {
                operation
                    .indexed_memory_cycle(cpu, effective, opcode)
                    .ok_or(CycleError::UnsupportedCycle)
            }
            _ => Err(CycleError::UnsupportedCycle),
        }
    }
}

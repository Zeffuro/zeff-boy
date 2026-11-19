use super::*;
use crate::state::{Reader, Writer, bounded_byte};

impl Operation {
    fn write_continuation(self, w: &mut Writer) {
        match self {
            Self::Read(value) => {
                w.write_u8(0);
                w.write_u8(value);
            }
            Self::Write(value) => {
                w.write_u8(1);
                w.write_u8(value);
            }
            Self::Alu(value) => {
                w.write_u8(2);
                w.write_u8(value);
            }
            Self::IncDec(value) => {
                w.write_u8(3);
                w.write_u8(value);
            }
            Self::Immediate => w.write_u8(4),
        }
    }

    fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        match r.read_u8()? {
            0 => {
                let register = bounded_byte(r, 7)?;
                anyhow::ensure!(
                    register != REGISTER_MEMORY_INDEX,
                    "invalid Z80 indexed register"
                );
                Ok(Self::Read(register))
            }
            1 => Ok(Self::Write(r.read_u8()?)),
            2 => Ok(Self::Alu(bounded_byte(r, 7)?)),
            3 => {
                let opcode = r.read_u8()?;
                anyhow::ensure!(
                    matches!(opcode, 0x34 | 0x35),
                    "invalid Z80 indexed RMW opcode"
                );
                Ok(Self::IncDec(opcode))
            }
            4 => Ok(Self::Immediate),
            _ => anyhow::bail!("invalid Z80 indexed operation tag"),
        }
    }
}

impl IndexedMemory {
    pub(crate) fn write_continuation(self, w: &mut Writer) {
        match self.0 {
            Phase::Displacement(operation, address) => {
                w.write_u8(0);
                operation.write_continuation(w);
                w.write_u16(address);
            }
            Phase::Address(operation, address) => {
                w.write_u8(1);
                operation.write_continuation(w);
                w.write_u16(address);
            }
            Phase::Immediate(address) => {
                w.write_u8(2);
                w.write_u16(address);
            }
            Phase::Memory(operation, address) => {
                w.write_u8(3);
                operation.write_continuation(w);
                w.write_u16(address);
            }
            Phase::Rmw(operation) => {
                w.write_u8(4);
                operation.write_continuation(w);
            }
        }
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        Ok(Self(match r.read_u8()? {
            0 => Phase::Displacement(Operation::read_continuation(r)?, r.read_u16()?),
            1 => {
                let operation = Operation::read_continuation(r)?;
                anyhow::ensure!(
                    operation != Operation::Immediate,
                    "invalid Z80 indexed address operation"
                );
                Phase::Address(operation, r.read_u16()?)
            }
            2 => Phase::Immediate(r.read_u16()?),
            3 => {
                let operation = Operation::read_continuation(r)?;
                anyhow::ensure!(
                    matches!(
                        operation,
                        Operation::Read(_) | Operation::Write(_) | Operation::Alu(_)
                    ),
                    "invalid Z80 indexed memory operation"
                );
                Phase::Memory(operation, r.read_u16()?)
            }
            4 => Phase::Rmw(rmw::Rmw::read_continuation(r)?),
            _ => anyhow::bail!("invalid Z80 indexed memory phase tag"),
        }))
    }
}

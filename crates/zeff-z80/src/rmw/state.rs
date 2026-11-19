use super::*;
use crate::state::{Reader, Writer, read_option_u8, write_option_u8};

impl Rmw {
    pub(crate) fn write_continuation(self, w: &mut Writer) {
        w.write_u16(self.address);
        write_option_u8(self.register, w);
        match self.phase {
            Phase::Read(operation) => {
                w.write_u8(0);
                match operation {
                    Operation::Increment => w.write_u8(0),
                    Operation::Decrement => w.write_u8(1),
                    Operation::Cb(opcode) => {
                        w.write_u8(2);
                        w.write_u8(opcode);
                    }
                    Operation::Decimal { opcode, a, carry } => {
                        w.write_u8(3);
                        w.write_u8(opcode);
                        w.write_u8(a);
                        w.write_u8(carry);
                    }
                }
            }
            Phase::DecimalInternal { value, flags, a } => {
                w.write_u8(1);
                w.write_u8(value);
                w.write_u8(flags);
                w.write_u8(a);
            }
            Phase::Write { value, flags, a } => {
                w.write_u8(2);
                w.write_u8(value);
                w.write_u8(flags);
                write_option_u8(a, w);
            }
        }
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        let address = r.read_u16()?;
        let register = read_option_u8(r)?;
        anyhow::ensure!(
            register.is_none_or(|value| value < 8),
            "invalid Z80 RMW register"
        );
        let phase = match r.read_u8()? {
            0 => Phase::Read(match r.read_u8()? {
                0 => Operation::Increment,
                1 => Operation::Decrement,
                2 => Operation::Cb(r.read_u8()?),
                3 => {
                    let opcode = r.read_u8()?;
                    let a = r.read_u8()?;
                    let carry = r.read_u8()?;
                    anyhow::ensure!(
                        matches!(opcode, 0x67 | 0x6f) && carry <= Z80_FLAG_CARRY,
                        "invalid Z80 decimal operation"
                    );
                    Operation::Decimal { opcode, a, carry }
                }
                _ => anyhow::bail!("invalid Z80 RMW operation tag"),
            }),
            1 => Phase::DecimalInternal {
                value: r.read_u8()?,
                flags: r.read_u8()?,
                a: r.read_u8()?,
            },
            2 => Phase::Write {
                value: r.read_u8()?,
                flags: r.read_u8()?,
                a: read_option_u8(r)?,
            },
            _ => anyhow::bail!("invalid Z80 RMW phase tag"),
        };
        Ok(Self {
            address,
            register,
            phase,
        })
    }
}

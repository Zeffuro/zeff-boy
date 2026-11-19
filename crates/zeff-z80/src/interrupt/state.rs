use super::*;
use crate::state::{Reader, Writer, read_option_u16, write_option_u16};

impl Service {
    pub(crate) fn write_continuation(self, w: &mut Writer) {
        w.write_u16(self.pc);
        match self.kind {
            Kind::Nmi => w.write_u8(0),
            Kind::Maskable(mode, i) => {
                w.write_u8(1);
                w.write_u8(interrupt_mode_to_byte(mode));
                w.write_u8(i);
            }
        }
        match self.phase {
            Phase::Acknowledge => w.write_u8(0),
            Phase::Load(operation) => {
                w.write_u8(1);
                operation.write_continuation(w);
            }
            Phase::Stack { operation, vector } => {
                w.write_u8(2);
                operation.write_continuation(w);
                write_option_u16(vector, w);
            }
            Phase::VectorLow(address) => {
                w.write_u8(3);
                w.write_u16(address);
            }
            Phase::VectorHigh(address, low) => {
                w.write_u8(4);
                w.write_u16(address);
                w.write_u8(low);
            }
        }
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        let pc = r.read_u16()?;
        let kind = match r.read_u8()? {
            0 => Kind::Nmi,
            1 => Kind::Maskable(byte_to_interrupt_mode(r.read_u8()?)?, r.read_u8()?),
            _ => anyhow::bail!("invalid Z80 interrupt kind"),
        };
        let phase = match r.read_u8()? {
            0 => Phase::Acknowledge,
            1 => Phase::Load(load::Load::read_continuation(r)?),
            2 => Phase::Stack {
                operation: stack::Stack::read_continuation(r)?,
                vector: read_option_u16(r)?,
            },
            3 => Phase::VectorLow(r.read_u16()?),
            4 => Phase::VectorHigh(r.read_u16()?, r.read_u8()?),
            _ => anyhow::bail!("invalid Z80 interrupt phase tag"),
        };
        Ok(Self { pc, kind, phase })
    }
}

use super::*;
use crate::state::{Reader, Writer};

impl IndexedCb {
    pub(crate) fn write_continuation(self, w: &mut Writer) {
        match self.0 {
            Phase::Displacement(address) => {
                w.write_u8(0);
                w.write_u16(address);
            }
            Phase::Terminal(address) => {
                w.write_u8(1);
                w.write_u16(address);
            }
            Phase::Memory(operation) => {
                w.write_u8(2);
                operation.write_continuation(w);
            }
        }
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        Ok(Self(match r.read_u8()? {
            0 => Phase::Displacement(r.read_u16()?),
            1 => Phase::Terminal(r.read_u16()?),
            2 => Phase::Memory(rmw::Rmw::read_continuation(r)?),
            _ => anyhow::bail!("invalid Z80 indexed CB phase tag"),
        }))
    }
}

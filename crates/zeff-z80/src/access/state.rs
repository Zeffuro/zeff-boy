use super::*;
use crate::state::{Reader, Writer};

impl Fetch {
    pub(crate) fn write_continuation(self, w: &mut Writer) {
        w.write_u8(match self {
            Self::Memory => 0,
            Self::Interrupt => 1,
        });
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        match r.read_u8()? {
            0 => Ok(Self::Memory),
            1 => Ok(Self::Interrupt),
            _ => anyhow::bail!("invalid Z80 fetch tag"),
        }
    }
}

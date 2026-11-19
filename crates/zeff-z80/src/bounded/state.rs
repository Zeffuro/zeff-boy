use super::*;
use crate::state::{Reader, Writer, read_index};

impl PrefixState {
    pub(crate) fn write_continuation(&self, w: &mut Writer) {
        w.write_bool(self.pending.is_some());
        if let Some(prefix) = self.pending {
            w.write_u16(prefix.pc);
            w.write_u8(prefix.selected);
        }
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            pending: if r.read_bool()? {
                Some(IndexPrefix {
                    pc: r.read_u16()?,
                    selected: read_index(r)?,
                })
            } else {
                None
            },
        })
    }
}

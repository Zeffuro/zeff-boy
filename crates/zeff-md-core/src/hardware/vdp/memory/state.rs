use super::*;
use crate::hardware::vdp::state::*;
use anyhow::{Result, ensure};

impl VdpMemory {
    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        w.write_bytes(&self.vram);
        for value in self.cram {
            w.write_u16(value);
        }
        for value in self.vsram {
            w.write_u16(value);
        }
    }

    pub(in crate::hardware::vdp) fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let mut result = Self::default();
        r.read_exact(&mut result.vram)?;
        for value in &mut result.cram {
            *value = r.read_u16()?;
            ensure!(*value & !CRAM_DATA_MASK == 0, "invalid VDP CRAM word");
        }
        for value in &mut result.vsram {
            *value = r.read_u16()?;
            ensure!(*value & !VSRAM_DATA_MASK == 0, "invalid VDP VSRAM word");
        }
        Ok(result)
    }
}

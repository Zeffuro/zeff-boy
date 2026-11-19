use super::*;
use crate::hardware::vdp::state::*;
use anyhow::{Result, ensure};

impl VdpReadCache {
    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        write_optional(w, self.data, StateWriter::write_u16);
        w.write_bool(self.ready);
    }

    pub(in crate::hardware::vdp) fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let result = Self {
            data: read_optional(r, StateReader::read_u16)?,
            ready: r.read_bool()?,
        };
        result.validate_state()?;
        Ok(result)
    }

    pub(in crate::hardware::vdp) fn validate_state(&self) -> Result<()> {
        ensure!(
            !self.ready || self.data.is_some(),
            "ready VDP read cache has no data"
        );
        Ok(())
    }
}

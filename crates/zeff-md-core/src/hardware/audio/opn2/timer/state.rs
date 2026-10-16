use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::StateReader as Reader;

impl LegacyTimer {
    pub(crate) fn read_state(r: &mut Reader<'_>, steps: u16, divider: u64) -> Result<Self> {
        let reload = r.read_u16()?;
        let remaining = r.read_u64()?;
        let running = r.read_bool()?;
        ensure!(reload < steps, "invalid FM timer reload");
        ensure!(
            remaining <= u64::from(steps) * divider && (!running || remaining != 0),
            "invalid FM timer remaining clocks"
        );
        Ok(Self {
            reload,
            remaining,
            running,
        })
    }
}

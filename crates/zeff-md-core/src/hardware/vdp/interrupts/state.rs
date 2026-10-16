use super::*;
use crate::hardware::vdp::state::*;
use anyhow::{Result, ensure};

impl VdpInterrupts {
    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        self.timing.write_state(w);
        w.write_u64(self.now.get());
        w.write_u8(self.line_counter);
        w.write_bool(self.horizontal_pending);
        w.write_bool(self.vertical_pending);
    }

    pub(in crate::hardware::vdp) fn read_state(
        r: &mut StateReader<'_>,
        version: u32,
        region: Region,
    ) -> Result<Self> {
        let mut value = Self {
            timing: VdpVerticalTiming::read_state(r, version)?,
            now: MasterTicks::new(r.read_u64()?),
            line_counter: r.read_u8()?,
            horizontal_pending: r.read_bool()?,
            vertical_pending: r.read_bool()?,
        };
        value.timing.migrate_calendar(value.now, region, version)?;
        Ok(value)
    }

    pub(in crate::hardware::vdp) fn validate_state(
        &self,
        registers: &VdpRegisterPipeline,
        region: Region,
    ) -> Result<()> {
        self.timing.validate_calendar(self.now, region)?;
        ensure!(
            self.now == registers.now(),
            "VDP register/interrupt clocks differ"
        );
        ensure!(
            self.timing
                .shares_internal_phase(registers.published_state().0),
            "VDP internal phases differ"
        );
        self.timing
            .hv_counter_at(self.now, region, &registers.published_state().1)
            .map_err(|error| anyhow::anyhow!("invalid VDP timing: {error:?}"))?;
        self.interrupt_level(registers)
            .map_err(|error| anyhow::anyhow!("invalid VDP interrupt image: {error:?}"))?;
        ensure!(
            registers.registers()[usize::from(HINT_RELOAD_REGISTER)].is_some(),
            "unknown VDP reload register"
        );
        Ok(())
    }
}

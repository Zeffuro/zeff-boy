use super::*;
use crate::hardware::vdp::state::*;
use anyhow::{Result, ensure};

impl VdpInterrupts {
    #[cfg(test)]
    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        self.write_core_state(w);
        self.write_blank_state(w)
            .expect("bounded VDP blank history section");
    }
    pub(in crate::hardware::vdp) fn write_core_state(&self, w: &mut StateWriter) {
        self.timing.write_state(w);
        w.write_u64(self.now.get());
        w.write_u8(self.line_counter);
        w.write_bool(self.horizontal_pending);
        w.write_bool(self.vertical_pending);
    }

    #[cfg(test)]
    pub(in crate::hardware::vdp) fn read_state(
        r: &mut StateReader<'_>,
        region: Region,
    ) -> Result<Self> {
        let mut value = Self::read_core_state(r, region)?;
        {
            value.read_blank_state(r)?;
        }
        Ok(value)
    }
    pub(in crate::hardware::vdp) fn read_core_state(
        r: &mut StateReader<'_>,
        _region: Region,
    ) -> Result<Self> {
        let value = Self {
            timing: VdpVerticalTiming::read_state(r)?,
            now: MasterTicks::new(r.read_u64()?),
            line_counter: r.read_u8()?,
            horizontal_pending: r.read_bool()?,
            vertical_pending: r.read_bool()?,
        };
        Ok(value)
    }

    pub(in crate::hardware::vdp) fn write_blank_state(&self, w: &mut StateWriter) -> Result<()> {
        crate::save_state::write_section(w, |w| {
            w.write_u8(3);
            w.write_bool(self.timing.blank.is_some());
            if let Some(blank) = self.timing.blank {
                blank.write_state(w);
            }
            Ok(())
        })
    }
    pub(in crate::hardware::vdp) fn read_blank_state(
        &mut self,
        r: &mut StateReader<'_>,
    ) -> Result<()> {
        let mut history = None;
        crate::save_state::read_section(r, |r| {
            ensure!(
                r.read_u8()? == { 3 },
                "unsupported VDP blank history section"
            );
            history = if r.read_bool()? {
                Some(super::super::timing::blank_mode::BlankHistory::read_state(
                    r,
                )?)
            } else {
                None
            };
            if let Some(blank) = history {
                blank.validate(self.now)?;
            }
            Ok(())
        })?;
        self.timing.blank = history;
        Ok(())
    }

    pub(in crate::hardware::vdp) fn validate_state(
        &self,
        registers: &VdpRegisterPipeline,
        region: Region,
    ) -> Result<()> {
        self.timing.validate_calendar(self.now, region)?;
        self.timing.validate_register_history(registers, region)?;
        if let Some(blank) = self.timing.blank.filter(|b| b.authoritative()) {
            ensure!(
                self.horizontal_pending == blank.core.hp
                    && self.vertical_pending == blank.core.vp
                    && self.line_counter == !blank.core.c,
                "VDP blank interrupt state mismatch"
            );
        }
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

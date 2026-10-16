use super::*;
use crate::hardware::vdp::state::*;
use crate::save_state::STATE_VERSION_WITH_INTERLACE;
use anyhow::{Result, ensure};

impl VdpVerticalTiming {
    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        w.write_u64(self.horizontal.epoch.get());
        w.write_u64(self.field.start.get());
        w.write_u64(self.field.id);
        w.write_u64(self.field.updated_at.get());
        w.write_bool(self.field.interlaced);
        w.write_bool(self.field.odd);
        w.write_bool(self.field.active_interlaced);
        w.write_bool(self.field.active_odd);
    }

    pub(in crate::hardware::vdp) fn read_state(
        r: &mut StateReader<'_>,
        version: u32,
    ) -> Result<Self> {
        let epoch = MasterTicks::new(r.read_u64()?);
        let field = if version >= STATE_VERSION_WITH_INTERLACE {
            field::FieldCalendar {
                start: MasterTicks::new(r.read_u64()?),
                id: r.read_u64()?,
                updated_at: MasterTicks::new(r.read_u64()?),
                interlaced: r.read_bool()?,
                odd: r.read_bool()?,
                active_interlaced: r.read_bool()?,
                active_odd: r.read_bool()?,
            }
        } else {
            field::FieldCalendar::new(epoch)
        };
        Ok(Self {
            horizontal: VdpH32Timing { epoch },
            field,
        })
    }

    pub(in crate::hardware::vdp) fn migrate_calendar(
        &mut self,
        now: MasterTicks,
        region: Region,
        version: u32,
    ) -> Result<()> {
        if version < STATE_VERSION_WITH_INTERLACE {
            let elapsed = self
                .horizontal
                .elapsed(now)
                .map_err(|error| anyhow::anyhow!("invalid VDP epoch: {error:?}"))?;
            let period = region.frame_lines() * H32_LINE_TICKS;
            self.field.id = elapsed / period;
            self.field.start = MasterTicks::new(now.get() - elapsed % period);
            self.field.updated_at = now;
        }
        Ok(())
    }

    pub(in crate::hardware::vdp) fn validate_calendar(
        &self,
        now: MasterTicks,
        region: Region,
    ) -> Result<()> {
        ensure!(self.field.updated_at == now, "VDP calendar clock differs");
        ensure!(
            self.field.start >= self.horizontal.epoch,
            "VDP field precedes epoch"
        );
        ensure!(
            (self.field.start.get() - self.horizontal.epoch.get()).is_multiple_of(H32_LINE_TICKS),
            "VDP field origin is not line aligned"
        );
        let lines = (self.field.start.get() - self.horizontal.epoch.get()) / H32_LINE_TICKS;
        let minimum = if region == Region::Pal { 312 } else { 262 };
        let maximum = if region == Region::Pal { 313 } else { 263 };
        ensure!(
            lines.div_ceil(maximum) <= self.field.id && self.field.id <= lines / minimum,
            "VDP field identity differs from origin"
        );
        ensure!(
            now.get()
                .checked_sub(self.field.start.get())
                .is_some_and(|elapsed| elapsed < self.field.lines(region) * H32_LINE_TICKS),
            "VDP field does not contain clock"
        );
        ensure!(
            self.field.interlaced || !self.field.odd,
            "progressive VDP odd flag"
        );
        ensure!(
            self.field.active_interlaced || !self.field.active_odd,
            "progressive VDP active parity"
        );
        Ok(())
    }
}

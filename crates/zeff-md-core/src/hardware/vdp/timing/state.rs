use super::*;
use crate::hardware::vdp::state::*;

use anyhow::{Result, ensure};

impl VdpVerticalTiming {
    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        w.write_u64(self.horizontal.epoch.get());
        w.write_u64(self.field.start.get());
        w.write_u64(self.field.id);
        w.write_u64(self.field.updated_at.get());
        self.field.mode.write_state(w);
        w.write_bool(self.field.odd);
        self.field.active_mode.write_state(w);
        w.write_bool(self.field.active_odd);
        write_optional(w, self.history, |w, history| {
            w.write_u64(history.cursor.start.get());
            w.write_u16(history.cursor.line);
            w.write_u8(match history.cursor.clock {
                clock::LineClock::H32 => 0,
                clock::LineClock::ExternalH40 => 1,
                clock::LineClock::InternalH40 => 2,
            });
            write_optional(w, history.pulse_end, |w, end| w.write_u64(end.get()));
        });
    }

    pub(in crate::hardware::vdp) fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let epoch = MasterTicks::new(r.read_u64()?);
        let field = {
            field::FieldCalendar {
                start: MasterTicks::new(r.read_u64()?),
                id: r.read_u64()?,
                updated_at: MasterTicks::new(r.read_u64()?),
                mode: InterlaceMode::read_state(r)?,
                odd: r.read_bool()?,
                active_mode: InterlaceMode::read_state(r)?,
                active_odd: r.read_bool()?,
            }
        };
        let history = {
            read_optional(r, |r| {
                let start = MasterTicks::new(r.read_u64()?);
                let line = r.read_u16()?;
                let clock = match r.read_u8()? {
                    0 => clock::LineClock::H32,
                    1 => clock::LineClock::ExternalH40,
                    2 => clock::LineClock::InternalH40,
                    _ => anyhow::bail!("invalid VDP line clock"),
                };
                Ok(history::TimingHistory {
                    cursor: cursor::LineCursor { start, line, clock },
                    pulse_end: read_optional(r, |r| Ok(MasterTicks::new(r.read_u64()?)))?,
                })
            })?
        };
        Ok(Self {
            horizontal: VdpH32Timing { epoch },
            field,
            history,
            blank: None,
        })
    }

    pub(in crate::hardware::vdp) fn validate_calendar(
        &self,
        now: MasterTicks,
        region: Region,
    ) -> Result<()> {
        ensure!(self.field.updated_at == now, "VDP calendar clock differs");
        if let Some(blank) = self.blank.filter(|b| b.authoritative()) {
            blank.validate(now)?;
            ensure!(
                self.history.is_none() && blank.core.epoch == self.horizontal.epoch,
                "conflicting VDP timing owners"
            );
            ensure!(
                self.field.id == blank.core.field_id && self.field.start == blank.core.field_start,
                "VDP blank field identity mismatch"
            );
            ensure!(
                self.field.mode == InterlaceMode::Progressive
                    && self.field.active_mode == InterlaceMode::Progressive
                    && !self.field.odd
                    && !self.field.active_odd,
                "invalid VDP blank field parity"
            );
            return Ok(());
        }
        if let Some(blank) = self.blank {
            blank.validate(now)?;
        }
        ensure!(
            self.field.start >= self.horizontal.epoch,
            "VDP field precedes epoch"
        );
        if let Some(history) = self.history {
            history
                .validate(self.field, region)
                .map_err(|error| anyhow::anyhow!("invalid VDP timing history: {error:?}"))?;
            if let Some(end) = history.pulse_end {
                let cursor = history.cursor;
                let onset_row = cursor.line == 224 || (region == Region::Pal && cursor.line == 240);
                let tail_row = cursor.line == 225 || (region == Region::Pal && cursor.line == 241);
                let onset = cursor
                    .start
                    .get()
                    .checked_add(cursor.clock.vertical_interrupt());
                let current = onset_row
                    && onset.is_some_and(|onset| {
                        onset <= now.get()
                            && onset.checked_add(cursor.clock.ticks()) == Some(end.get())
                    });
                let tail = tail_row
                    && [H32_VINT_COUNTER * H32_DOT_TICKS, u64::from(H40_DOT_TICKS)]
                        .into_iter()
                        .any(|offset| cursor.start.get().checked_add(offset) == Some(end.get()));
                ensure!(current || tail, "VDP sound pulse differs from cursor");
            }
            let elapsed = self.field.start.get() - self.horizontal.epoch.get();
            ensure!(
                elapsed.is_multiple_of(clock::LINE_PHASE_TICKS),
                "VDP anchored field origin is not line aligned"
            );
            let minimum =
                if region == Region::Pal { 312 } else { 262 } * clock::INTERNAL_LINE_TICKS;
            let maximum = if region == Region::Pal { 313 } else { 263 } * H32_LINE_TICKS;
            ensure!(
                elapsed.div_ceil(maximum) <= self.field.id && self.field.id <= elapsed / minimum,
                "VDP anchored field identity differs from origin"
            );
            history
                .cursor
                .stable_field_end(self.field, now, region)
                .map_err(|error| anyhow::anyhow!("invalid VDP field deadline: {error:?}"))?;
            return Ok(());
        }
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
            self.field.mode.interlaced() || !self.field.odd,
            "progressive VDP odd flag"
        );
        ensure!(
            self.field.active_mode.interlaced() || !self.field.active_odd,
            "progressive VDP active parity"
        );
        Ok(())
    }

    pub(in crate::hardware::vdp) fn validate_register_history(
        &self,
        registers: &VdpRegisterPipeline,
        region: Region,
    ) -> Result<()> {
        let published = registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)];
        if let Some(blank) = self.blank {
            blank.validate_registers(registers, region)?;
        }
        if let Some(blank) = self.blank.filter(|b| b.authoritative()) {
            if blank.vertical.is_some() {
                ensure!(
                    blank.vertical_supported(registers, region),
                    "invalid M5 vertical profile"
                );
                return Ok(());
            }
            ensure!(
                blank.eligible
                    && registers.registers()[0].is_some_and(|r| r & 1 == 0)
                    && registers.registers()[1]
                        .is_some_and(|r| r & (DISPLAY_ENABLE | V30_ENABLE | VRAM_128K_ENABLE) == 0)
                    && published == Some(0),
                "unsupported VDP blank history registers"
            );
            return Ok(());
        }
        let Some(history) = self.history else {
            ensure!(
                !published.is_some_and(|value| value & H40_CLOCK_MASK == 1),
                "VDP internal clock has no timing history"
            );
            return Ok(());
        };
        history::HistoryProjection::new(self.field, history, *registers, region)
            .map_err(|error| anyhow::anyhow!("invalid VDP history registers: {error:?}"))?;
        if self.field.updated_at == history.cursor.start {
            ensure!(
                published.and_then(|value| clock::LineClock::from_register(value).ok())
                    == Some(history.cursor.clock),
                "VDP HC0 clock differs from published selection"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

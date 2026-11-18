use super::clock::LineClock;
use super::*;
use field::FieldCalendar;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct LineCursor {
    pub(in crate::hardware::vdp) start: MasterTicks,
    pub(in crate::hardware::vdp) line: u16,
    pub(in crate::hardware::vdp) clock: LineClock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct CompletedLine {
    pub(in crate::hardware::vdp) at: MasterTicks,
    pub(in crate::hardware::vdp) field: u64,
    pub(in crate::hardware::vdp) field_start: MasterTicks,
    pub(in crate::hardware::vdp) line_start: MasterTicks,
    pub(in crate::hardware::vdp) line: u16,
    pub(in crate::hardware::vdp) clock: LineClock,
    pub(in crate::hardware::vdp) wrapped: bool,
}

impl LineCursor {
    pub(in crate::hardware::vdp) fn from_legacy(
        field: FieldCalendar,
        at: MasterTicks,
        region: Region,
        clock: LineClock,
    ) -> Result<Self, VdpTimingError> {
        if clock == LineClock::InternalH40 || field.updated_at != at {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        let elapsed =
            at.get()
                .checked_sub(field.start.get())
                .ok_or(VdpTimingError::BeforeEpoch {
                    epoch: field.start,
                    requested: at,
                })?;
        if elapsed >= region.frame_lines() * H32_LINE_TICKS {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        let cursor = Self {
            start: MasterTicks::new(at.get() - elapsed % H32_LINE_TICKS),
            line: (elapsed / H32_LINE_TICKS) as u16,
            clock,
        };
        cursor.validate(field, at, region)?;
        Ok(cursor)
    }

    pub(in crate::hardware::vdp) fn next_boundary(self) -> Result<MasterTicks, VdpTimingError> {
        self.start
            .get()
            .checked_add(self.clock.ticks())
            .map(MasterTicks::new)
            .ok_or(VdpTimingError::ClockOverflow)
    }

    pub(in crate::hardware::vdp) fn phase(self, at: MasterTicks) -> Result<u64, VdpTimingError> {
        let phase = at
            .get()
            .checked_sub(self.start.get())
            .ok_or(VdpTimingError::BeforeEpoch {
                epoch: self.start,
                requested: at,
            })?;
        if phase >= self.clock.ticks() {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        Ok(phase)
    }

    pub(in crate::hardware::vdp) fn validate(
        self,
        field: FieldCalendar,
        at: MasterTicks,
        region: Region,
    ) -> Result<(), VdpTimingError> {
        self.phase(at)?;
        self.next_boundary()?;
        let prefix = self
            .start
            .get()
            .checked_sub(field.start.get())
            .ok_or(VdpTimingError::UnqualifiedDisplayHistory)?;
        let line = u64::from(self.line);
        if field.mode.interlaced()
            || field.active_mode.interlaced()
            || field.odd
            || field.active_odd
            || line >= region.frame_lines()
            || field.updated_at != at
            || !(line * clock::INTERNAL_LINE_TICKS..=line * H32_LINE_TICKS).contains(&prefix)
            || !(prefix - line * clock::INTERNAL_LINE_TICKS)
                .is_multiple_of(H32_LINE_TICKS - clock::INTERNAL_LINE_TICKS)
        {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        Ok(())
    }

    pub(in crate::hardware::vdp) fn stable_field_end(
        self,
        field: FieldCalendar,
        at: MasterTicks,
        region: Region,
    ) -> Result<MasterTicks, VdpTimingError> {
        self.validate(field, at, region)?;
        let remaining = region.frame_lines() - u64::from(self.line);
        self.start
            .get()
            .checked_add(remaining * self.clock.ticks())
            .map(MasterTicks::new)
            .ok_or(VdpTimingError::ClockOverflow)
    }

    pub(in crate::hardware::vdp) fn finish_line(
        &mut self,
        field: &mut FieldCalendar,
        region: Region,
        next_clock: LineClock,
    ) -> Result<CompletedLine, VdpTimingError> {
        self.validate(*field, field.updated_at, region)?;
        let at = self.next_boundary()?;
        at.get()
            .checked_add(next_clock.ticks())
            .ok_or(VdpTimingError::ClockOverflow)?;
        let wrapped = u64::from(self.line) + 1 == region.frame_lines();
        let id = if wrapped {
            field
                .id
                .checked_add(1)
                .ok_or(VdpTimingError::ClockOverflow)?
        } else {
            field.id
        };
        let completed = CompletedLine {
            at,
            field: field.id,
            field_start: field.start,
            line_start: self.start,
            line: self.line,
            clock: self.clock,
            wrapped,
        };
        if wrapped {
            field.start = at;
        }
        field.id = id;
        field.updated_at = at;
        self.start = at;
        self.line = if wrapped { 0 } else { self.line + 1 };
        self.clock = next_clock;
        Ok(completed)
    }
}

#[cfg(test)]
mod tests;

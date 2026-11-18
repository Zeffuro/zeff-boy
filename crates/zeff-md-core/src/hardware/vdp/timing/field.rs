use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldPosition {
    pub id: u64,
    pub start: MasterTicks,
    pub lines: u64,
    pub line: u16,
    pub line_start: MasterTicks,
    pub line_ticks: u64,
    pub projected_end: Option<MasterTicks>,
    pub mode: InterlaceMode,
    pub odd: bool,
    pub active_mode: InterlaceMode,
    pub active_odd: bool,
}

impl FieldPosition {
    pub(crate) fn end(self) -> Result<MasterTicks, VdpTimingError> {
        self.projected_end.ok_or(VdpTimingError::ClockOverflow)
    }

    pub(crate) fn next_line(self) -> Result<MasterTicks, VdpTimingError> {
        self.line_start
            .get()
            .checked_add(self.line_ticks)
            .map(MasterTicks::new)
            .ok_or(VdpTimingError::ClockOverflow)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct FieldCalendar {
    pub start: MasterTicks,
    pub id: u64,
    pub updated_at: MasterTicks,
    pub mode: InterlaceMode,
    pub odd: bool,
    pub active_mode: InterlaceMode,
    pub active_odd: bool,
}

impl FieldCalendar {
    pub(in crate::hardware::vdp) fn new(epoch: MasterTicks) -> Self {
        Self {
            start: epoch,
            id: 0,
            updated_at: epoch,
            mode: InterlaceMode::Progressive,
            odd: false,
            active_mode: InterlaceMode::Progressive,
            active_odd: false,
        }
    }

    pub(in crate::hardware::vdp) fn lines(self, region: Region) -> u64 {
        if self.mode.interlaced() {
            (if region == Region::Pal { 312 } else { 262 }) + u64::from(self.odd)
        } else {
            region.frame_lines()
        }
    }

    fn tick(self, offset: u64) -> Result<MasterTicks, VdpTimingError> {
        self.start
            .get()
            .checked_add(offset)
            .map(MasterTicks::new)
            .ok_or(VdpTimingError::ClockOverflow)
    }

    pub(in crate::hardware::vdp) fn end(
        self,
        region: Region,
    ) -> Result<MasterTicks, VdpTimingError> {
        self.tick(self.lines(region) * H32_LINE_TICKS)
    }

    pub(in crate::hardware::vdp) fn position(
        self,
        at: MasterTicks,
        region: Region,
    ) -> Result<FieldPosition, VdpTimingError> {
        let elapsed =
            at.get()
                .checked_sub(self.start.get())
                .ok_or(VdpTimingError::BeforeEpoch {
                    epoch: self.start,
                    requested: at,
                })?;
        Ok(FieldPosition {
            id: self.id,
            start: self.start,
            lines: self.lines(region),
            line: (elapsed / H32_LINE_TICKS) as u16,
            line_start: MasterTicks::new(at.get() - elapsed % H32_LINE_TICKS),
            line_ticks: H32_LINE_TICKS,
            projected_end: self.end(region).ok(),
            mode: self.mode,
            odd: self.odd,
            active_mode: self.active_mode,
            active_odd: self.active_odd,
        })
    }

    #[inline]
    fn project_current(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &impl CalendarRegisters,
    ) -> Result<bool, VdpTimingError> {
        if at < self.updated_at {
            return Err(VdpTimingError::BeforeEpoch {
                epoch: self.updated_at,
                requested: at,
            });
        }
        if at == self.updated_at {
            return Ok(true);
        }
        if !self.mode.interlaced()
            && registers.pending_publication().is_none()
            && registers
                .size_byte()
                .is_some_and(|value| value & INTERLACE_MODE_MASK == 0)
        {
            let count = (at.get() - self.start.get()) / (region.frame_lines() * H32_LINE_TICKS);
            if count != 0 {
                self.start = self.tick(count * region.frame_lines() * H32_LINE_TICKS)?;
                self.id = self
                    .id
                    .checked_add(count)
                    .ok_or(VdpTimingError::ClockOverflow)?;
                self.active_mode = InterlaceMode::Progressive;
                self.active_odd = false;
            }
            self.updated_at = at;
            return Ok(true);
        }
        Ok(false)
    }

    #[inline]
    pub(in crate::hardware::vdp) fn project_field(
        mut self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<Self, VdpTimingError> {
        if self.project_current(at, region, registers)? {
            return Ok(self);
        }
        self.project_events(at, region, registers)
            .map(|projected| projected.0)
    }

    #[inline]
    pub(in crate::hardware::vdp) fn project<R: CalendarRegisters>(
        mut self,
        at: MasterTicks,
        region: Region,
        registers: &R,
    ) -> Result<(Self, R), VdpTimingError> {
        if self.project_current(at, region, registers)? {
            return Ok((self, *registers));
        }
        self.project_events(at, region, registers)
    }

    fn project_events<R: CalendarRegisters>(
        mut self,
        at: MasterTicks,
        region: Region,
        registers: &R,
    ) -> Result<(Self, R), VdpTimingError> {
        let mut registers = *registers;
        loop {
            let mode = horizontal_mode_byte(registers.size_byte())?;
            let requested = registers.size_byte().unwrap();
            let end = self.end(region).ok();
            let publication = registers.pending_publication();
            if !self.mode.interlaced()
                && requested & INTERLACE_MODE_MASK == 0
                && end.is_some_and(|end| at >= end)
            {
                let horizon = publication.map_or(at, |tick| tick.min(at));
                let period = region.frame_lines() * H32_LINE_TICKS;
                let count = (horizon.get() - self.start.get()) / period;
                if count != 0 {
                    self.start = self.tick(count * period)?;
                    self.id = self
                        .id
                        .checked_add(count)
                        .ok_or(VdpTimingError::ClockOverflow)?;
                    self.active_mode = InterlaceMode::Progressive;
                    self.active_odd = false;
                    self.updated_at = self.start;
                    if publication.is_some_and(|tick| tick <= self.start) {
                        registers
                            .advance_calendar_to(self.start)
                            .map_err(|_| VdpTimingError::PendingModeChange)?;
                    }
                    continue;
                }
            }
            let height = display_height_byte(registers.mode_byte(), region) as u64;
            let latch = self
                .tick(VdpVerticalTiming::blank_offset(
                    height,
                    mode.vertical_clock(),
                ))
                .ok();
            let toggle = self
                .tick(height * H32_LINE_TICKS + mode.vertical_interrupt())
                .ok();
            let next = [latch, toggle, end, publication]
                .into_iter()
                .flatten()
                .filter(|&tick| tick > self.updated_at)
                .min();
            let Some(next) = next.filter(|&tick| tick <= at) else {
                self.updated_at = at;
                return Ok((self, registers));
            };
            if Some(next) == latch {
                let mode = InterlaceMode::from_register(requested).unwrap();
                let enabled = mode.interlaced();
                if enabled != self.mode.interlaced() {
                    self.odd = false;
                }
                self.mode = mode;
            }
            if Some(next) == toggle && self.mode.interlaced() {
                self.odd = !self.odd;
            }
            if Some(next) == end {
                self.start = next;
                self.id = self
                    .id
                    .checked_add(1)
                    .ok_or(VdpTimingError::ClockOverflow)?;
                self.active_mode = self.mode;
                self.active_odd = self.odd;
            }
            // The published image at the edge owns a coincident field latch.
            if publication == Some(next) {
                registers
                    .advance_calendar_to(next)
                    .map_err(|_| VdpTimingError::PendingModeChange)?;
            }
            self.updated_at = next;
        }
    }
}

impl VdpVerticalTiming {
    pub(in crate::hardware::vdp) fn sound_calendar_at<R: CalendarRegisters>(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &R,
    ) -> Result<(FieldCalendar, R), VdpTimingError> {
        self.horizontal.elapsed(at)?;
        if self.history.is_some() {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        let mut field = self.field;
        if at < field.updated_at
            && !field.mode.interlaced()
            && !field.active_mode.interlaced()
            && registers
                .size_byte()
                .is_some_and(|value| value & INTERLACE_MODE_MASK == 0)
        {
            let period = region.frame_lines() * H32_LINE_TICKS;
            // Regional field-length deviations have one sign, so they cannot cancel.
            let linear = field.id.checked_mul(period).is_some_and(|elapsed| {
                Some(elapsed) == field.start.get().checked_sub(self.horizontal.epoch.get())
            });
            if linear {
                field = FieldCalendar::new(self.horizontal.epoch);
            } else if at >= field.start {
                field.updated_at = at;
            }
        }
        field.project(at, region, registers)
    }

    pub(crate) fn field_position_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<FieldPosition, VdpTimingError> {
        self.horizontal.elapsed(at)?;
        if let Some((history, _)) = self.vertical_at(at, region, registers)?
            && history.vertical.is_some()
        {
            return history.vertical_position();
        }
        if let Some(blank) = self.blank.filter(|b| b.authoritative()) {
            let mut view = *registers;
            let projected = blank.advance_projection(at, region, &mut view)?;
            return if projected.vertical.is_some() {
                projected.vertical_position()
            } else {
                Ok(projected.field_position(region))
            };
        }
        if self.uses_history(registers) {
            return self.anchored_field_position_at(at, region, registers);
        }
        self.field
            .project_field(at, region, registers)?
            .position(at, region)
    }

    pub(in crate::hardware::vdp) fn advance_calendar(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<(), VdpTimingError> {
        if self.blank.is_some_and(|b| b.authoritative()) {
            return Ok(());
        }
        if self.uses_history(registers) {
            return self.advance_anchored_calendar(at, region, registers);
        }
        self.field = self.field.project_field(at, region, registers)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[allow(dead_code)]
pub(in crate::hardware::vdp) mod oracle;

#[cfg(test)]
mod projection_tests;

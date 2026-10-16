use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldPosition {
    pub id: u64,
    pub start: MasterTicks,
    pub lines: u64,
    pub line: u16,
    pub interlaced: bool,
    pub odd: bool,
    pub active_interlaced: bool,
    pub active_odd: bool,
}

impl FieldPosition {
    pub(crate) fn end(self) -> Result<MasterTicks, VdpTimingError> {
        self.start
            .get()
            .checked_add(self.lines * H32_LINE_TICKS)
            .map(MasterTicks::new)
            .ok_or(VdpTimingError::ClockOverflow)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct FieldCalendar {
    pub start: MasterTicks,
    pub id: u64,
    pub updated_at: MasterTicks,
    pub interlaced: bool,
    pub odd: bool,
    pub active_interlaced: bool,
    pub active_odd: bool,
}

impl FieldCalendar {
    pub(in crate::hardware::vdp) fn new(epoch: MasterTicks) -> Self {
        Self {
            start: epoch,
            id: 0,
            updated_at: epoch,
            interlaced: false,
            odd: false,
            active_interlaced: false,
            active_odd: false,
        }
    }

    pub(in crate::hardware::vdp) fn lines(self, region: Region) -> u64 {
        if self.interlaced {
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
            interlaced: self.interlaced,
            odd: self.odd,
            active_interlaced: self.active_interlaced,
            active_odd: self.active_odd,
        })
    }

    #[inline]
    fn project_current(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
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
        if !self.interlaced
            && registers.pending().is_none()
            && registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
                .is_some_and(|value| value & INTERLACE_MODE_MASK == 0)
        {
            let count = (at.get() - self.start.get()) / (region.frame_lines() * H32_LINE_TICKS);
            if count != 0 {
                self.start = self.tick(count * region.frame_lines() * H32_LINE_TICKS)?;
                self.id = self
                    .id
                    .checked_add(count)
                    .ok_or(VdpTimingError::ClockOverflow)?;
                self.active_interlaced = false;
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
    pub(in crate::hardware::vdp) fn project(
        mut self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<(Self, VdpRegisterPipeline), VdpTimingError> {
        if self.project_current(at, region, registers)? {
            return Ok((self, *registers));
        }
        self.project_events(at, region, registers)
    }

    fn project_events(
        mut self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<(Self, VdpRegisterPipeline), VdpTimingError> {
        let mut registers = *registers;
        loop {
            let published = registers.published_state().1;
            let mode = VdpVerticalTiming::horizontal_mode(&published)?;
            let requested = published.registers()[usize::from(DISPLAY_SIZE_REGISTER)].unwrap();
            let end = self.end(region).ok();
            let publication = registers.pending().map(|write| write.publish_at);
            if !self.interlaced
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
                    self.active_interlaced = false;
                    self.active_odd = false;
                    self.updated_at = self.start;
                    if publication.is_some_and(|tick| tick <= self.start) {
                        registers
                            .advance_to(self.start)
                            .map_err(|_| VdpTimingError::PendingModeChange)?;
                    }
                    continue;
                }
            }
            let height = published.display_height(region) as u64;
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
                let enabled = requested & INTERLACE_MODE_MASK == INTERLACE_MODE_MASK;
                if enabled != self.interlaced {
                    self.odd = false;
                }
                self.interlaced = enabled;
            }
            if Some(next) == toggle && self.interlaced {
                self.odd = !self.odd;
            }
            if Some(next) == end {
                self.start = next;
                self.id = self
                    .id
                    .checked_add(1)
                    .ok_or(VdpTimingError::ClockOverflow)?;
                self.active_interlaced = self.interlaced;
                self.active_odd = self.odd;
            }
            // The published image at the edge owns a coincident field latch.
            if publication == Some(next) {
                registers
                    .advance_to(next)
                    .map_err(|_| VdpTimingError::PendingModeChange)?;
            }
            self.updated_at = next;
        }
    }
}

impl VdpVerticalTiming {
    pub(in crate::hardware::vdp) fn sound_calendar_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<(FieldCalendar, VdpRegisterPipeline), VdpTimingError> {
        self.horizontal.elapsed(at)?;
        let mut field = self.field;
        if at < field.updated_at
            && !field.interlaced
            && !field.active_interlaced
            && registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
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
        self.field = self.field.project_field(at, region, registers)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

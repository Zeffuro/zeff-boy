use super::history::{HistoryProjection, TimingHistory};
use super::*;

impl VdpVerticalTiming {
    pub(in crate::hardware::vdp) fn next_scroll_deadline(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpTimingError> {
        let position = self.field_position_at(at, region, registers)?;
        let fetch = self.scroll_fetch_at(at, region, registers)?;
        Ok(if fetch > at {
            fetch
        } else if self.blank.is_some_and(|b| b.vertical.is_some()) {
            // Re-evaluate the selected clock at HC0 before scheduling the next fetch.
            position.next_line()?
        } else {
            MasterTicks::new(
                fetch
                    .get()
                    .checked_add(position.line_ticks)
                    .ok_or(VdpTimingError::ClockOverflow)?,
            )
        })
    }

    pub(in crate::hardware::vdp) fn scroll_fetch_phase(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<u64, VdpTimingError> {
        let position = self.field_position_at(at, region, registers)?;
        Ok(self.scroll_fetch_at(at, region, registers)?.get() - position.line_start.get())
    }

    pub(in crate::hardware::vdp) fn scroll_fetch_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpTimingError> {
        let position = self.field_position_at(at, region, registers)?;
        let clock = if let Some((history, _)) = self.vertical_at(at, region, registers)? {
            history.vertical.map_or(clock::LineClock::H32, |v| v.clock)
        } else if let Some(projection) = self.anchored_at(at, region, registers)? {
            projection.history.cursor.clock
        } else {
            super::clock::LineClock::from_register(
                registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
                    .ok_or(VdpTimingError::UnknownRegister(DISPLAY_SIZE_REGISTER))?,
            )?
        };
        let fetch = position
            .line_start
            .get()
            .checked_add(clock.scroll_fetch_phase())
            .ok_or(VdpTimingError::ClockOverflow)?;
        Ok(MasterTicks::new(fetch))
    }

    pub(in crate::hardware::vdp) fn next_row_boundary(
        &self,
        at: MasterTicks,
    ) -> Result<MasterTicks, VdpTimingError> {
        if let Some(vertical) = self.blank.and_then(|b| b.vertical) {
            return vertical.boundary();
        }
        if let Some(history) = self.history {
            history.cursor.phase(at)?;
            return history.cursor.next_boundary();
        }
        let phase = self.horizontal.elapsed(at)? % H32_LINE_TICKS;
        at.get()
            .checked_add(H32_LINE_TICKS - phase)
            .map(MasterTicks::new)
            .ok_or(VdpTimingError::ClockOverflow)
    }

    #[inline]
    pub(in crate::hardware::vdp) fn uses_history(
        &self,
        registers: &impl CalendarRegisters,
    ) -> bool {
        self.blank.is_none_or(|b| !b.authoritative())
            && (self.history.is_some() || registers.pending_internal_clock())
    }

    pub(in crate::hardware::vdp) fn anchored_projection<R: CalendarRegisters>(
        &self,
        region: Region,
        registers: &R,
    ) -> Result<Option<HistoryProjection<R>>, VdpTimingError> {
        if !self.uses_history(registers) {
            return Ok(None);
        }
        let (field, history) = if let Some(history) = self.history {
            (self.field, history)
        } else {
            let field = self
                .field
                .project(registers.calendar_now(), region, registers)?
                .0;
            (field, TimingHistory::from_legacy(field, region, registers)?)
        };
        HistoryProjection::new(field, history, *registers, region).map(Some)
    }

    pub(in crate::hardware::vdp) fn anchored_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<Option<HistoryProjection<VdpRegisterPipeline>>, VdpTimingError> {
        self.anchored_projection(region, registers)?
            .map(|projection| projection.project(at))
            .transpose()
    }

    #[inline(never)]
    pub(super) fn anchored_hv_counter_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<u16, VdpTimingError> {
        self.anchored_at(at, region, registers)?
            .unwrap()
            .hv_counter_now()
    }

    #[inline(never)]
    pub(super) fn anchored_counter_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<u16, VdpTimingError> {
        self.anchored_at(at, region, registers)?
            .unwrap()
            .vcounter_now()
    }

    #[inline(never)]
    pub(super) fn anchored_vblank_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpTimingError> {
        self.anchored_at(at, region, registers)?
            .unwrap()
            .vblank_now()
    }

    #[inline(never)]
    pub(super) fn anchored_next_vblank(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<VdpVBlankEdge, VdpTimingError> {
        self.anchored_at(after, region, registers)?
            .unwrap()
            .next_vblank()
    }

    #[inline(never)]
    pub(super) fn anchored_next_interrupt(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<VdpInterruptEdge, VdpTimingError> {
        self.anchored_at(after, region, registers)?
            .unwrap()
            .next_interrupt()
    }

    #[inline(never)]
    pub(super) fn anchored_field_position_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<FieldPosition, VdpTimingError> {
        self.anchored_at(at, region, registers)?
            .unwrap()
            .field_position()
    }

    #[inline(never)]
    pub(super) fn advance_anchored_calendar(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<(), VdpTimingError> {
        if let Some(projected) = self.anchored_at(at, region, registers)?
            && (self.history.is_some()
                || projected
                    .registers
                    .size_byte()
                    .is_some_and(|value| value & H40_CLOCK_MASK == 1))
        {
            self.field = projected.field;
            self.history = Some(projected.history);
            return Ok(());
        }
        self.field = self.field.project_field(at, region, registers)?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn history_projection(
        &self,
        registers: &VdpRegisterPipeline,
        region: Region,
        history: Option<TimingHistory>,
    ) -> Result<HistoryProjection<VdpRegisterPipeline>, VdpTimingError> {
        let field = if history.is_some() {
            self.field
        } else {
            self.field
                .project_field(registers.now(), region, registers)?
        };
        let history = match history {
            Some(history) => history,
            None => TimingHistory::from_legacy(field, region, registers)?,
        };
        HistoryProjection::new(field, history, *registers, region)
    }
}

impl<R: CalendarRegisters> HistoryProjection<R> {
    pub(in crate::hardware::vdp) fn field_position(self) -> Result<FieldPosition, VdpTimingError> {
        let cursor = self.history.cursor;
        Ok(FieldPosition {
            id: self.field.id,
            start: self.field.start,
            lines: self.field.lines(self.region),
            line: cursor.line,
            line_start: cursor.start,
            line_ticks: cursor.clock.ticks(),
            projected_end: self.field_end().ok(),
            mode: self.field.mode,
            odd: self.field.odd,
            active_mode: self.field.active_mode,
            active_odd: self.field.active_odd,
        })
    }
}

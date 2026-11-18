use super::*;

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct HistoryPosition {
    pub(in crate::hardware::vdp) id: u64,
    pub(in crate::hardware::vdp) field_start: MasterTicks,
    pub(in crate::hardware::vdp) lines: u64,
    pub(in crate::hardware::vdp) line: u16,
    pub(in crate::hardware::vdp) line_start: MasterTicks,
    pub(in crate::hardware::vdp) phase: u64,
    pub(in crate::hardware::vdp) clock: LineClock,
}

impl<R: CalendarRegisters> HistoryProjection<R> {
    #[cfg(test)]
    pub(in crate::hardware::vdp) fn position(&self) -> Result<HistoryPosition, VdpTimingError> {
        self.history.validate(self.field, self.region)?;
        Ok(HistoryPosition {
            id: self.field.id,
            field_start: self.field.start,
            lines: self.field.lines(self.region),
            line: self.history.cursor.line,
            line_start: self.history.cursor.start,
            phase: self.history.cursor.phase(self.field.updated_at)?,
            clock: self.history.cursor.clock,
        })
    }

    pub(in crate::hardware::vdp) fn hcounter_now(&self) -> Result<u16, VdpTimingError> {
        self.history
            .cursor
            .clock
            .counter(self.history.cursor.phase(self.field.updated_at)?)
    }

    fn counter_line(&self) -> Result<u64, VdpTimingError> {
        let cursor = self.history.cursor;
        let phase = cursor.phase(self.field.updated_at)?;
        Ok(
            (u64::from(cursor.line) + u64::from(phase >= cursor.clock.vertical_clock()))
                % self.field.lines(self.region),
        )
    }

    pub(in crate::hardware::vdp) fn vcounter_now(&self) -> Result<u16, VdpTimingError> {
        let line = self.counter_line()?;
        let (low_count, high_start) =
            if display_height_byte(self.registers.mode_byte(), self.region) == 240 {
                (PAL_V30_COUNTER_LOW_COUNT, PAL_V30_COUNTER_HIGH_START)
            } else if self.region == Region::Pal {
                (PAL_V28_COUNTER_LOW_COUNT, PAL_V28_COUNTER_HIGH_START)
            } else {
                (NTSC_V28_COUNTER_LOW_COUNT, NTSC_V28_COUNTER_HIGH_START)
            };
        Ok(if line < low_count {
            line as u16
        } else {
            (line - low_count + high_start) as u16
        })
    }

    #[cfg(test)]
    pub(in crate::hardware::vdp) fn hblank_now(&self) -> Result<bool, VdpTimingError> {
        self.history
            .cursor
            .clock
            .blank(self.history.cursor.phase(self.field.updated_at)?)
    }

    pub(in crate::hardware::vdp) fn hv_counter_now(&self) -> Result<u16, VdpTimingError> {
        Ok(u16::from_be_bytes([
            self.vcounter_now()? as u8,
            (self.hcounter_now()? >> HV_HCOUNTER_SHIFT) as u8,
        ]))
    }

    pub(in crate::hardware::vdp) fn vblank_now(&self) -> Result<bool, VdpTimingError> {
        let height = display_height_byte(self.registers.mode_byte(), self.region) as u64;
        Ok((height..self.field.lines(self.region) - 1).contains(&self.counter_line()?))
    }

    pub(in crate::hardware::vdp) fn sound_now(&self) -> bool {
        self.history
            .pulse_end
            .is_some_and(|end| self.field.updated_at < end)
    }

    fn memory_active(&self) -> bool {
        self.registers
            .mode_byte()
            .is_some_and(|value| value & DISPLAY_ENABLE != 0)
            && usize::from(self.history.cursor.line)
                < display_height_byte(self.registers.mode_byte(), self.region)
    }

    pub(in crate::hardware::vdp) fn slot_now(&self) -> Result<bool, VdpTimingError> {
        self.history.cursor.clock.is_slot(
            self.history.cursor.phase(self.field.updated_at)?,
            self.memory_active(),
        )
    }

    pub(in crate::hardware::vdp) fn next_slot(self) -> Result<MasterTicks, VdpTimingError> {
        self.next_slot_in_mode(false)
    }

    pub(in crate::hardware::vdp) fn next_slot_in_mode(
        mut self,
        external: bool,
    ) -> Result<MasterTicks, VdpTimingError> {
        loop {
            let phase = self.history.cursor.phase(self.field.updated_at)?;
            let slot = self
                .history
                .cursor
                .clock
                .next_slot(phase, !external && self.memory_active())?
                .map(|offset| self.tick(offset))
                .transpose()?;
            let event = self.next_event()?;
            if let Some(slot) = slot.filter(|&slot| slot < event) {
                return Ok(slot);
            }
            let event = self.step()?;
            if if external {
                self.history
                    .cursor
                    .clock
                    .is_slot(self.history.cursor.phase(event.at)?, false)?
            } else {
                event.slot
            } {
                return Ok(event.at);
            }
        }
    }

    pub(in crate::hardware::vdp) fn field_end(mut self) -> Result<MasterTicks, VdpTimingError> {
        self.field
            .id
            .checked_add(1)
            .ok_or(VdpTimingError::ClockOverflow)?;
        loop {
            if self.registers.pending_publication().is_none() {
                let remaining =
                    self.field.lines(self.region) - u64::from(self.history.cursor.line) - 1;
                let next_clock = requested_clock(&self.registers, self.region)?;
                return self
                    .history
                    .cursor
                    .next_boundary()?
                    .get()
                    .checked_add(remaining * next_clock.ticks())
                    .map(MasterTicks::new)
                    .ok_or(VdpTimingError::ClockOverflow);
            }
            let event = self.step()?;
            if event.completed.is_some_and(|line| line.wrapped) {
                return Ok(event.at);
            }
        }
    }

    pub(in crate::hardware::vdp) fn next_interrupt(
        mut self,
    ) -> Result<VdpInterruptEdge, VdpTimingError> {
        loop {
            let event = self.step()?;
            if event.horizontal || event.vertical {
                return Ok(VdpInterruptEdge {
                    at: event.at,
                    clock: if event.vertical {
                        VdpInterruptClock::Vertical
                    } else {
                        VdpInterruptClock::HorizontalCounter
                    },
                });
            }
        }
    }

    pub(in crate::hardware::vdp) fn next_vblank(mut self) -> Result<VdpVBlankEdge, VdpTimingError> {
        let active = self.vblank_now()?;
        loop {
            let event = self.step()?;
            let next = self.vblank_now()?;
            if next != active {
                return Ok(VdpVBlankEdge {
                    at: event.at,
                    active: next,
                });
            }
        }
    }

    #[cfg(test)]
    pub(in crate::hardware::vdp) fn next_hblank(mut self) -> Result<VdpHBlankEdge, VdpTimingError> {
        let active = self.hblank_now()?;
        loop {
            let event = self.step()?;
            let next = self.hblank_now()?;
            if next != active {
                return Ok(VdpHBlankEdge {
                    at: event.at,
                    active: next,
                });
            }
        }
    }
}

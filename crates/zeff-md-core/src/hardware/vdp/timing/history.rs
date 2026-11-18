use super::clock::LineClock;
use super::cursor::{CompletedLine, LineCursor};
use super::field::FieldCalendar;
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct TimingHistory {
    pub(in crate::hardware::vdp) cursor: LineCursor,
    pub(in crate::hardware::vdp) pulse_end: Option<MasterTicks>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct HistoryProjection<R> {
    pub(in crate::hardware::vdp) field: FieldCalendar,
    pub(in crate::hardware::vdp) history: TimingHistory,
    pub(in crate::hardware::vdp) registers: R,
    pub(in crate::hardware::vdp) region: Region,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct HistoryEvent {
    pub(in crate::hardware::vdp) at: MasterTicks,
    pub(in crate::hardware::vdp) completed: Option<CompletedLine>,
    pub(in crate::hardware::vdp) publication: bool,
    pub(in crate::hardware::vdp) horizontal: bool,
    pub(in crate::hardware::vdp) vertical: bool,
    pub(in crate::hardware::vdp) pulse_ended: bool,
    pub(in crate::hardware::vdp) slot: bool,
}

impl TimingHistory {
    pub(in crate::hardware::vdp) fn from_legacy(
        field: FieldCalendar,
        region: Region,
        registers: &impl CalendarRegisters,
    ) -> Result<Self, VdpTimingError> {
        // Seed before publication, while the legacy pulse interval is still authoritative.
        let clock = requested_clock(registers, region)?;
        let cursor = LineCursor::from_legacy(field, field.updated_at, region, clock)?;
        let onset = field
            .start
            .get()
            .checked_add(display_height_byte(registers.mode_byte(), region) as u64 * H32_LINE_TICKS)
            .and_then(|at| at.checked_add(clock.vertical_interrupt()))
            .ok_or(VdpTimingError::ClockOverflow)?;
        let pulse_end = if onset <= field.updated_at.get() {
            let end = onset
                .checked_add(H32_LINE_TICKS)
                .ok_or(VdpTimingError::ClockOverflow)?;
            (field.updated_at.get() < end).then_some(MasterTicks::new(end))
        } else {
            None
        };
        Ok(Self { cursor, pulse_end })
    }

    pub(in crate::hardware::vdp) fn validate(
        self,
        field: FieldCalendar,
        region: Region,
    ) -> Result<(), VdpTimingError> {
        self.cursor.validate(field, field.updated_at, region)?;
        if let Some(end) = self.pulse_end
            && !end
                .get()
                .checked_sub(field.updated_at.get())
                .is_some_and(|remaining| (1..=H32_LINE_TICKS).contains(&remaining))
        {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        Ok(())
    }
}

fn requested_clock(
    registers: &impl CalendarRegisters,
    region: Region,
) -> Result<LineClock, VdpTimingError> {
    validate_mode_byte(registers.mode_byte(), region, false)?;
    let r1 = registers.mode_byte().unwrap();
    if r1 & VRAM_128K_ENABLE != 0 {
        return Err(VdpTimingError::UnsupportedMode {
            index: MODE_REGISTER,
            value: r1,
        });
    }
    let value = registers
        .size_byte()
        .ok_or(VdpTimingError::UnknownRegister(DISPLAY_SIZE_REGISTER))?;
    let clock = LineClock::from_register(value)?;
    if value & INTERLACE_MODE_MASK != 0 {
        return Err(VdpTimingError::UnqualifiedDisplayHistory);
    }
    Ok(clock)
}

impl<R: CalendarRegisters> HistoryProjection<R> {
    pub(in crate::hardware::vdp) fn new(
        field: FieldCalendar,
        history: TimingHistory,
        registers: R,
        region: Region,
    ) -> Result<Self, VdpTimingError> {
        history.validate(field, region)?;
        requested_clock(&registers, region)?;
        if let Some(at) = registers.pending_publication() {
            if at <= field.updated_at {
                return Err(VdpTimingError::PendingModeChange);
            }
            let mut future = registers;
            future
                .advance_calendar_to(at)
                .map_err(|_| VdpTimingError::PendingModeChange)?;
            requested_clock(&future, region)?;
        }
        Ok(Self {
            field,
            history,
            registers,
            region,
        })
    }

    fn tick(&self, offset: u64) -> Result<MasterTicks, VdpTimingError> {
        self.history
            .cursor
            .start
            .get()
            .checked_add(offset)
            .map(MasterTicks::new)
            .ok_or(VdpTimingError::ClockOverflow)
    }

    fn onset(&self) -> Result<Option<MasterTicks>, VdpTimingError> {
        if usize::from(self.history.cursor.line)
            == display_height_byte(self.registers.mode_byte(), self.region)
        {
            self.tick(self.history.cursor.clock.vertical_interrupt())
                .map(Some)
        } else {
            Ok(None)
        }
    }

    pub(in crate::hardware::vdp) fn next_event(&self) -> Result<MasterTicks, VdpTimingError> {
        let clock = self.history.cursor.clock;
        let (clear, set) = if clock == LineClock::H32 {
            (
                u64::from(H32_HBLANK_CLEAR_COUNTER) * H32_DOT_TICKS,
                u64::from(H32_HBLANK_SET_COUNTER) * H32_DOT_TICKS,
            )
        } else {
            (H40_HBLANK_CLEAR_TICKS, H40_HBLANK_SET_TICKS)
        };
        [
            Some(self.history.cursor.next_boundary()?),
            Some(self.tick(clock.vertical_clock())?),
            Some(self.tick(clear)?),
            Some(self.tick(set)?),
            self.onset()?,
            self.registers.pending_publication(),
            self.history.pulse_end,
        ]
        .into_iter()
        .flatten()
        .filter(|&at| at > self.field.updated_at)
        .min()
        .ok_or(VdpTimingError::ClockOverflow)
    }

    pub(in crate::hardware::vdp) fn step(&mut self) -> Result<HistoryEvent, VdpTimingError> {
        let mut projected = *self;
        let event = projected.step_inner()?;
        *self = projected;
        Ok(event)
    }

    fn step_inner(&mut self) -> Result<HistoryEvent, VdpTimingError> {
        self.history.validate(self.field, self.region)?;
        requested_clock(&self.registers, self.region)?;
        let at = self.next_event()?;
        let boundary = self.history.cursor.next_boundary()?;
        let publication = self.registers.pending_publication() == Some(at);
        let horizontal = self.tick(self.history.cursor.clock.vertical_clock())? == at;
        let vertical = self.onset()? == Some(at);
        let pulse_ended = self.history.pulse_end == Some(at);
        // Preview the new image for checked HC0 admission; old-edge IRQs use the old image.
        let mut published = self.registers;
        published
            .advance_calendar_to(at)
            .map_err(|_| VdpTimingError::PendingModeChange)?;
        let requested = requested_clock(&published, self.region)?;
        if pulse_ended {
            self.history.pulse_end = None;
        }
        if vertical {
            self.history.pulse_end = Some(MasterTicks::new(
                at.get()
                    .checked_add(self.history.cursor.clock.ticks())
                    .ok_or(VdpTimingError::ClockOverflow)?,
            ));
        }
        let completed = if at == boundary {
            Some(
                self.history
                    .cursor
                    .finish_line(&mut self.field, self.region, requested)?,
            )
        } else {
            self.field.updated_at = at;
            None
        };
        self.registers = published;
        self.history.validate(self.field, self.region)?;
        Ok(HistoryEvent {
            at,
            completed,
            publication,
            horizontal,
            vertical,
            pulse_ended,
            slot: self.slot_now()?,
        })
    }

    pub(in crate::hardware::vdp) fn advance_to(
        &mut self,
        at: MasterTicks,
    ) -> Result<(), VdpTimingError> {
        if at < self.field.updated_at {
            return Err(VdpTimingError::BeforeEpoch {
                epoch: self.field.updated_at,
                requested: at,
            });
        }
        let mut projected = *self;
        while projected.next_event()? <= at {
            projected.step()?;
        }
        projected
            .registers
            .advance_calendar_to(at)
            .map_err(|_| VdpTimingError::PendingModeChange)?;
        projected.field.updated_at = at;
        projected
            .history
            .validate(projected.field, projected.region)?;
        *self = projected;
        Ok(())
    }

    pub(in crate::hardware::vdp) fn project(
        mut self,
        at: MasterTicks,
    ) -> Result<Self, VdpTimingError> {
        self.advance_to(at)?;
        Ok(self)
    }
}

mod queries;
#[cfg(test)]
mod tests;

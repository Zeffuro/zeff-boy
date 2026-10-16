use super::constants::*;
use super::{
    VdpInterruptClock, VdpInterruptEdge, VdpRegisterPipeline, VdpTimingError, VdpVerticalTiming,
};
use crate::Region;
use zeff_emu_common::time::MasterTicks;

mod schedule;
mod sound_irq;
mod state;
pub use schedule::{VdpEvent, VdpScheduleError};
pub(crate) use sound_irq::SoundInterrupt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpInterruptError {
    Timing(VdpTimingError),
    TimeReversal {
        current: MasterTicks,
        requested: MasterTicks,
    },
    PendingReload,
}

impl From<VdpTimingError> for VdpInterruptError {
    fn from(error: VdpTimingError) -> Self {
        Self::Timing(error)
    }
}

/// Requires known HC0/VC0 and counter/latch seeds, stable mode and collision-free operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpInterrupts {
    timing: VdpVerticalTiming,
    now: MasterTicks,
    line_counter: u8,
    horizontal_pending: bool,
    vertical_pending: bool,
}

impl VdpInterrupts {
    pub(crate) fn field_count(&self) -> u64 {
        self.timing.field.id
    }

    pub(crate) fn field_position(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<super::timing::FieldPosition, VdpTimingError> {
        self.field_position_at(self.now, region, registers)
    }

    pub(crate) fn field_position_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<super::timing::FieldPosition, VdpTimingError> {
        self.timing.field_position_at(at, region, registers)
    }

    pub(crate) fn next_field_end(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpTimingError> {
        self.field_position(region, registers)
            .and_then(|position| position.end())
    }

    pub(crate) fn completed_row(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<Option<(u16, super::timing::FieldPosition)>, VdpTimingError> {
        let position = self.field_position_at(at, region, registers)?;
        let elapsed = at.get() - position.start.get();
        Ok(
            (elapsed.is_multiple_of(H32_LINE_TICKS) && position.line > 0)
                .then_some((position.line.saturating_sub(1), position))
                .filter(|(row, _)| usize::from(*row) < registers.display_height(region)),
        )
    }

    pub(crate) fn progressive_skip_allowed(&self, registers: &VdpRegisterPipeline) -> bool {
        registers.pending().is_none()
            && !self.timing.field.interlaced
            && !self.timing.field.active_interlaced
            && registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
                .is_some_and(|value| value & INTERLACE_MODE_MASK == 0)
    }

    pub(super) fn is_external_slot(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpScheduleError> {
        self.timing
            .is_external_fetch_slot(at, region, registers)
            .map_err(|error| VdpInterruptError::from(error).into())
    }
    pub(super) fn next_external_slot(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpScheduleError> {
        self.timing
            .next_external_fetch_slot(after, region, registers)
            .map_err(|error| VdpInterruptError::from(error).into())
    }
    pub(super) fn vblank_flag(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpScheduleError> {
        self.timing
            .vblank_in_mode(self.now, region, registers, false)
            .map_err(|error| VdpInterruptError::from(error).into())
    }
    pub(super) fn hv_counter(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<u16, VdpScheduleError> {
        self.timing
            .hv_counter_at(self.now, region, registers)
            .map_err(|error| VdpInterruptError::from(error).into())
    }

    pub(super) fn display_line(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<u16, VdpScheduleError> {
        self.timing
            .elapsed_in_mode(self.now, region, registers, true)
            .map_err(VdpInterruptError::from)?;
        self.field_position(region, registers)
            .map(|position| position.line)
            .map_err(|error| VdpInterruptError::from(error).into())
    }

    pub(super) fn next_write_slot(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpScheduleError> {
        self.timing
            .next_machine_memory_slot(after, region, registers)
            .map_err(|error| VdpInterruptError::from(error).into())
    }

    pub(super) fn is_write_slot(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpScheduleError> {
        self.timing
            .is_memory_slot(at, region, registers)
            .map_err(|error| VdpInterruptError::from(error).into())
    }

    pub(super) fn is_machine_write_slot(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpScheduleError> {
        self.timing
            .is_machine_memory_slot(at, region, registers)
            .map_err(|error| VdpInterruptError::from(error).into())
    }

    pub(super) fn timing_next_edge(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<Option<VdpInterruptEdge>, VdpScheduleError> {
        match self
            .timing
            .next_counter_edge(self.now, region, &registers.published_state().1, false)
        {
            Ok(edge) => Ok(Some(edge)),
            Err(VdpTimingError::ClockOverflow) => Ok(None),
            Err(error) => Err(VdpInterruptError::from(error).into()),
        }
    }

    pub(super) fn cycle_boundary(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<Option<MasterTicks>, VdpScheduleError> {
        match self.next_field_end(region, &registers.published_state().1) {
            Ok(end) => Ok(Some(end)),
            Err(VdpTimingError::ClockOverflow) => Ok(None),
            Err(error) => Err(VdpInterruptError::from(error).into()),
        }
    }

    /// Whole cycles preserve the settled line counter; pending requests coalesce.
    pub(super) fn skip_cycles(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<Option<MasterTicks>, VdpScheduleError> {
        if !self.progressive_skip_allowed(registers) {
            return Ok(None);
        }
        self.timing
            .elapsed_in_mode(self.now, region, registers, false)
            .map_err(VdpInterruptError::from)?;
        let period = region.frame_lines() * H32_LINE_TICKS;
        let reload = published(registers, HINT_RELOAD_REGISTER)?;
        let remaining =
            at.get()
                .checked_sub(self.now.get())
                .ok_or(VdpInterruptError::TimeReversal {
                    current: self.now,
                    requested: at,
                })?;
        let cycles = remaining / period;
        let position = self
            .field_position(region, registers)
            .map_err(VdpInterruptError::from)?;
        if self.now != position.start
            || cycles == 0
            || self.line_counter != reload.saturating_sub(1)
        {
            return Ok(None);
        }
        self.now = MasterTicks::new(self.now.get() + cycles * period);
        self.timing
            .advance_calendar(self.now, region, registers)
            .map_err(VdpInterruptError::from)?;
        self.horizontal_pending |= u64::from(reload) <= registers.display_height(region) as u64;
        self.vertical_pending = true;
        Ok(Some(self.now))
    }

    pub fn at_counters_zero(
        epoch: MasterTicks,
        line_counter: u8,
        horizontal_pending: bool,
        vertical_pending: bool,
    ) -> Self {
        Self {
            timing: VdpVerticalTiming::at_counters_zero(epoch),
            now: epoch,
            line_counter,
            horizontal_pending,
            vertical_pending,
        }
    }

    pub fn now(&self) -> MasterTicks {
        self.now
    }
    pub fn line_counter(&self) -> u8 {
        self.line_counter
    }
    pub fn horizontal_pending(&self) -> bool {
        self.horizontal_pending
    }
    pub fn vertical_pending(&self) -> bool {
        self.vertical_pending
    }
    pub fn status_bits(&self) -> u16 {
        (u16::from(self.timing.field.interlaced && self.timing.field.odd) * ODD_FIELD_STATUS)
            | if self.vertical_pending {
                VINT_PENDING_STATUS
            } else {
                0
            }
    }

    pub(super) fn hblank_flag(
        &self,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpScheduleError> {
        self.timing
            .hblank_flag_at(self.now, registers)
            .map_err(|error| VdpInterruptError::from(error).into())
    }

    pub fn interrupt_level(
        &self,
        registers: &VdpRegisterPipeline,
    ) -> Result<u8, VdpInterruptError> {
        let horizontal = published(registers, HINT_ENABLE_REGISTER)? & HINT_ENABLE_MASK != 0;
        let vertical = published(registers, MODE_REGISTER)? & VINT_ENABLE_MASK != 0;
        Ok(if vertical && self.vertical_pending {
            VINT_LEVEL
        } else if horizontal && self.horizontal_pending {
            HINT_LEVEL
        } else {
            0
        })
    }

    /// Isolated IACK selects current VDP output, not the CPU's earlier IPL sample.
    pub fn complete_acknowledge(
        &mut self,
        registers: &VdpRegisterPipeline,
    ) -> Result<u8, VdpInterruptError> {
        let level = self.interrupt_level(registers)?;
        match level {
            VINT_LEVEL => self.vertical_pending = false,
            HINT_LEVEL => self.horizontal_pending = false,
            _ => {}
        }
        Ok(level)
    }

    /// Returns one due event; repeat until None. Keep R10 stable and order writes between calls.
    pub fn advance_to(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<Option<VdpInterruptEdge>, VdpInterruptError> {
        self.advance_in_mode(at, region, registers, true)
    }

    pub(super) fn advance_in_mode(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
        require_display: bool,
    ) -> Result<Option<VdpInterruptEdge>, VdpInterruptError> {
        self.advance_with_edge(at, region, registers, require_display, |timing, now| {
            timing.next_counter_edge(now, region, registers, require_display)
        })
    }

    fn advance_with_edge(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
        require_display: bool,
        next: impl FnOnce(&VdpVerticalTiming, MasterTicks) -> Result<VdpInterruptEdge, VdpTimingError>,
    ) -> Result<Option<VdpInterruptEdge>, VdpInterruptError> {
        if at < self.now {
            return Err(VdpInterruptError::TimeReversal {
                current: self.now,
                requested: at,
            });
        }
        self.timing
            .elapsed_in_mode(at, region, registers, require_display)?;
        if registers
            .pending()
            .is_some_and(|write| write.index == HINT_RELOAD_REGISTER)
        {
            return Err(VdpInterruptError::PendingReload);
        }
        let reload = published(registers, HINT_RELOAD_REGISTER)?;
        let edge = match next(&self.timing, self.now) {
            Ok(edge) if edge.at <= at => edge,
            Ok(_) | Err(VdpTimingError::ClockOverflow) => {
                self.timing.advance_calendar(at, region, registers)?;
                self.now = at;
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        match edge.clock {
            VdpInterruptClock::HorizontalCounter => {
                let counter =
                    self.timing
                        .counter_in_mode(edge.at, region, registers, require_display)?;
                if usize::from(counter) <= registers.display_height(region) {
                    if let Some(value) = self.line_counter.checked_sub(1) {
                        self.line_counter = value;
                    } else {
                        self.line_counter = reload;
                        self.horizontal_pending = true;
                    }
                } else {
                    self.line_counter = reload;
                }
            }
            VdpInterruptClock::Vertical => self.vertical_pending = true,
        }
        self.timing.advance_calendar(edge.at, region, registers)?;
        self.now = edge.at;
        Ok(Some(edge))
    }
}

fn published(registers: &VdpRegisterPipeline, index: u8) -> Result<u8, VdpInterruptError> {
    registers.registers()[usize::from(index)].ok_or(VdpTimingError::UnknownRegister(index).into())
}

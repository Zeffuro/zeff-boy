use super::constants::*;
use super::{InterlaceMode, VdpRegisterPipeline};
use crate::Region;
use zeff_emu_common::time::MasterTicks;

mod calendar;
mod field;
mod horizontal;
pub(crate) use field::FieldPosition;
pub(in crate::hardware::vdp) mod blank_mode;
mod register_view;
mod state;
use horizontal::horizontal_mode_byte;
use register_view::CalendarRegisters;
pub(in crate::hardware::vdp) use register_view::SoundRegisters;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpTimingError {
    BeforeEpoch {
        epoch: MasterTicks,
        requested: MasterTicks,
    },
    ClockOverflow,
    UnknownRegister(u8),
    UnsupportedMode {
        index: u8,
        value: u8,
    },
    UnsupportedRegion(Region),
    PendingModeChange,
    UnqualifiedMemoryInterval(MasterTicks),
    UnqualifiedDisplayHistory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpVBlankEdge {
    pub at: MasterTicks,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpInterruptClock {
    HorizontalCounter,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpInterruptEdge {
    pub at: MasterTicks,
    pub clock: VdpInterruptClock,
}

/// Requires a known HC0/VC0 phase and stable latched mode; phase is not reset or raster origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpVerticalTiming {
    horizontal: VdpH32Timing,
    pub(super) history: Option<history::TimingHistory>,
    pub(super) field: field::FieldCalendar,
    pub(in crate::hardware::vdp) blank: Option<blank_mode::BlankHistory>,
}

impl VdpVerticalTiming {
    pub(super) fn hblank_flag_at(
        &self,
        at: MasterTicks,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpTimingError> {
        if let Some(blank) = self.blank.filter(|b| b.authoritative()) {
            let mut view = *registers;
            if blank.vertical.is_some() {
                let projected = blank.advance_projection(at, Region::NtscUs, &mut view)?;
                return projected.hblank(at, view.registers()[1].unwrap());
            }
            view.advance_to(at)
                .map_err(|_| VdpTimingError::PendingModeChange)?;
            return blank.hblank(
                at,
                view.registers()[1].ok_or(VdpTimingError::UnknownRegister(1))?,
            );
        }
        if let Some(history) = self.history {
            return history.cursor.clock.blank(history.cursor.phase(at)?);
        }
        let phase = self.horizontal.elapsed(at)? % H32_LINE_TICKS;
        Ok(Self::horizontal_mode(registers)?.blank(phase))
    }

    pub(super) fn is_write_slot(&self, at: MasterTicks) -> Result<bool, VdpTimingError> {
        Ok(
            self.horizontal.elapsed(at)? % H32_MEMORY_SLOT_TICKS == H32_DOT_TICKS
                && !H32_NONACTIVE_REFRESH.contains(&self.horizontal.h_counter_at(at)?),
        )
    }

    pub(super) fn shares_internal_phase(&self, epoch: MasterTicks) -> bool {
        self.horizontal.epoch.get() % H32_INTERNAL_CLOCK_TICKS
            == epoch.get() % H32_INTERNAL_CLOCK_TICKS
    }

    pub fn at_counters_zero(epoch: MasterTicks) -> Self {
        Self {
            horizontal: VdpH32Timing::at_counter_zero(epoch),
            history: None,
            field: field::FieldCalendar::new(epoch),
            blank: None,
        }
    }

    pub fn v_counter_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<u16, VdpTimingError> {
        self.counter_in_mode(at, region, registers, true)
    }

    pub(super) fn hv_counter_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<u16, VdpTimingError> {
        if let Some((history, _)) = self.vertical_at(at, region, registers)? {
            let horizontal = if let Some(clock) = history.vertical {
                clock.counter(at)?
            } else {
                history.core.h_counter(at)
            };
            return Ok(u16::from_be_bytes([
                history.core.v as u8,
                (horizontal >> HV_HCOUNTER_SHIFT) as u8,
            ]));
        }
        if self.blank.is_some_and(|b| b.authoritative()) {
            let vertical = self.counter_in_mode(at, region, registers, false)?;
            let horizontal = if let Some(vertical) = self.blank.unwrap().vertical {
                vertical.counter(at)?
            } else {
                self.horizontal.h_counter_at(at)?
            } >> HV_HCOUNTER_SHIFT;
            return Ok(u16::from_be_bytes([vertical as u8, horizontal as u8]));
        }
        if self.uses_history(registers) {
            return self.anchored_hv_counter_at(at, region, registers);
        }
        let mut vertical = self.counter_in_mode(at, region, registers, false)?;
        vertical = match self.field_position_at(at, region, registers)?.mode {
            InterlaceMode::Progressive => vertical,
            InterlaceMode::Normal => (vertical & 0xfe) | ((vertical >> 8) & 1),
            InterlaceMode::Double => u16::from((vertical as u8).rotate_left(1)),
        };
        let phase = self.horizontal.elapsed(at)? % H32_LINE_TICKS;
        let horizontal = Self::horizontal_mode(registers)?.counter(phase) >> HV_HCOUNTER_SHIFT;
        Ok(u16::from_be_bytes([vertical as u8, horizontal as u8]))
    }

    pub(super) fn counter_in_mode(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
        require_display: bool,
    ) -> Result<u16, VdpTimingError> {
        if let Some((history, _)) = self.vertical_at(at, region, registers)? {
            return Ok(history.core.v);
        }
        self.elapsed_in_mode(at, region, registers, require_display)?;
        if let Some(blank) = self.blank.filter(|b| b.authoritative()) {
            let mut view = *registers;
            return Ok(blank.advance_projection(at, region, &mut view)?.core.v);
        }
        if self.uses_history(registers) {
            return self.anchored_counter_at(at, region, registers);
        }
        let position = self.field_position_at(at, region, registers)?;
        let elapsed = at.get() - position.start.get();
        let vertical_clock = Self::horizontal_mode(registers)?.vertical_clock();
        let lines = position.lines;
        let line = (u64::from(position.line)
            + u64::from(elapsed % H32_LINE_TICKS >= vertical_clock))
            % lines;
        let (mut low_count, mut high_start) =
            if registers.display_height(region) == V30_VBLANK_SET_LINE as usize {
                (PAL_V30_COUNTER_LOW_COUNT, PAL_V30_COUNTER_HIGH_START)
            } else if region == Region::Pal {
                (PAL_V28_COUNTER_LOW_COUNT, PAL_V28_COUNTER_HIGH_START)
            } else {
                (NTSC_V28_COUNTER_LOW_COUNT, NTSC_V28_COUNTER_HIGH_START)
            };
        if position.mode.interlaced() {
            if region == Region::Pal {
                low_count -= 1;
            }
            high_start = if region == Region::Pal {
                if registers.display_height(region) == V30_VBLANK_SET_LINE as usize {
                    0x1d2
                } else {
                    0x1ca
                }
            } else {
                0x1e5
            } - u64::from(position.odd);
        }
        Ok(if line < low_count {
            line as u16
        } else {
            (line - low_count + high_start) as u16
        })
    }

    pub fn vblank_flag_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpTimingError> {
        self.vblank_in_mode(at, region, registers, true)
    }

    pub(super) fn vblank_in_mode(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
        require_display: bool,
    ) -> Result<bool, VdpTimingError> {
        if let Some((history, regs)) = self.vertical_at(at, region, registers)? {
            return Ok(regs.registers()[1].unwrap() & DISPLAY_ENABLE == 0
                || !(history.core.active || history.core.last));
        }
        self.elapsed_in_mode(at, region, registers, require_display)?;
        if let Some(blank) = self.blank.filter(|b| b.authoritative()) {
            let mut view = *registers;
            let projected = blank.advance_projection(at, region, &mut view)?;
            return Ok(view.registers()[1].unwrap() & DISPLAY_ENABLE == 0
                || !(projected.core.last || projected.core.active));
        }
        if self.uses_history(registers) {
            return self.anchored_vblank_at(at, region, registers);
        }
        let position = self.field_position_at(at, region, registers)?;
        let phase = at.get() - position.start.get();
        let lines = position.lines;
        let clock = Self::horizontal_mode(registers)?.vertical_clock();
        Ok(
            (Self::blank_offset(registers.display_height(region) as u64, clock)
                ..Self::blank_offset(lines - 1, clock))
                .contains(&phase),
        )
    }

    pub fn next_vblank_edge(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<VdpVBlankEdge, VdpTimingError> {
        if let Some(history) = self.vertical_source(region, registers) {
            return history.next_vertical_blank(after, region, *registers);
        }
        if self.blank.is_some_and(|b| b.authoritative()) {
            return Err(VdpTimingError::ClockOverflow);
        }
        self.qualified_elapsed(after, region, registers)?;
        if self.uses_history(registers) {
            return self.anchored_next_vblank(after, region, registers);
        }
        let position = self.field_position_at(after, region, registers)?;
        let phase = after.get() - position.start.get();
        let mode = Self::horizontal_mode(registers)?;
        let set = Self::blank_offset(
            registers.display_height(region) as u64,
            mode.vertical_clock(),
        );
        if phase < set {
            return Ok(VdpVBlankEdge {
                at: MasterTicks::new(
                    position
                        .start
                        .get()
                        .checked_add(set)
                        .ok_or(VdpTimingError::ClockOverflow)?,
                ),
                active: true,
            });
        }
        let toggle_offset =
            registers.display_height(region) as u64 * H32_LINE_TICKS + mode.vertical_interrupt();
        let toggle = position
            .start
            .get()
            .checked_add(toggle_offset)
            .ok_or(VdpTimingError::ClockOverflow)?;
        let settled = self.field.project_field(
            MasterTicks::new(after.get().max(toggle)),
            region,
            registers,
        )?;
        let period = settled.lines(region) * H32_LINE_TICKS;
        let clear = Self::blank_offset(period / H32_LINE_TICKS - 1, mode.vertical_clock());
        let (step, active) = if phase < clear {
            (clear - phase, false)
        } else {
            (period - phase + set, true)
        };
        let at = after
            .get()
            .checked_add(step)
            .ok_or(VdpTimingError::ClockOverflow)?;
        Ok(VdpVBlankEdge {
            at: MasterTicks::new(at),
            active,
        })
    }

    pub fn next_interrupt_edge(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<VdpInterruptEdge, VdpTimingError> {
        self.next_counter_edge(after, region, registers, true)
    }

    pub(super) fn next_counter_edge(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
        require_display: bool,
    ) -> Result<VdpInterruptEdge, VdpTimingError> {
        let mut position = None;
        self.next_counter_edge_with_position(
            after,
            region,
            registers,
            require_display,
            &mut position,
        )
    }

    pub(in crate::hardware::vdp) fn next_counter_edge_with_position(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
        require_display: bool,
        position: &mut Option<FieldPosition>,
    ) -> Result<VdpInterruptEdge, VdpTimingError> {
        if let Some(history) = self.vertical_source(region, registers) {
            return history.next_interrupt(after, region, registers);
        }
        if let Some(blank) = self.blank.filter(|b| b.authoritative()) {
            return blank.next_interrupt(after, region, registers);
        }
        let elapsed = self.elapsed_in_mode(after, region, registers, require_display)?;
        if self.uses_history(registers) {
            return self.anchored_next_interrupt(after, region, registers);
        }
        let line_phase = elapsed % H32_LINE_TICKS;
        let mode = Self::horizontal_mode(registers)?;
        let horizontal = mode.vertical_clock();
        let horizontal_step = if line_phase < horizontal {
            horizontal - line_phase
        } else {
            H32_LINE_TICKS - line_phase + horizontal
        };
        let position = match position.take() {
            Some(position) => position,
            None => self.field_position_at(after, region, registers)?,
        };
        let period = position.lines * H32_LINE_TICKS;
        let phase = after.get() - position.start.get();
        let vertical =
            registers.display_height(region) as u64 * H32_LINE_TICKS + mode.vertical_interrupt();
        let vertical_step = if phase < vertical {
            vertical - phase
        } else {
            period - phase + vertical
        };
        let (step, clock) = if horizontal_step < vertical_step {
            (horizontal_step, VdpInterruptClock::HorizontalCounter)
        } else {
            (vertical_step, VdpInterruptClock::Vertical)
        };
        let at = after
            .get()
            .checked_add(step)
            .ok_or(VdpTimingError::ClockOverflow)?;
        Ok(VdpInterruptEdge {
            at: MasterTicks::new(at),
            clock,
        })
    }

    fn blank_offset(line: u64, clock: u64) -> u64 {
        (line - 1) * H32_LINE_TICKS + clock
    }

    fn qualified_elapsed(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<u64, VdpTimingError> {
        self.elapsed_in_mode(at, region, registers, true)
    }

    pub(super) fn elapsed_in_mode(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
        require_display: bool,
    ) -> Result<u64, VdpTimingError> {
        let elapsed = self.horizontal.elapsed(at)?;
        if let Some(blank) = self.blank.filter(|b| b.authoritative()) {
            if blank.vertical.is_some() {
                if !blank.vertical_supported(registers, region) {
                    return Err(VdpTimingError::UnqualifiedDisplayHistory);
                }
                return Ok(elapsed);
            }
            if !blank.eligible
                || registers.registers()[1]
                    .is_none_or(|r| r & (DISPLAY_ENABLE | V30_ENABLE | VRAM_128K_ENABLE) != 0)
                || registers.registers()[0].is_none_or(|r| r & 1 != 0)
                || registers.registers()[12] != Some(0)
            {
                return Err(VdpTimingError::UnqualifiedDisplayHistory);
            }
            return Ok(elapsed);
        }
        if registers.pending_memory_mode_change() && !self.uses_history(registers) {
            return Err(VdpTimingError::PendingModeChange);
        }
        validate_mode_byte(
            registers.registers()[usize::from(MODE_REGISTER)],
            region,
            require_display,
        )?;
        if self.uses_history(registers) {
            clock::LineClock::from_register(
                registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
                    .ok_or(VdpTimingError::UnknownRegister(DISPLAY_SIZE_REGISTER))?,
            )?;
        } else {
            Self::horizontal_mode(registers)?;
        }
        Ok(elapsed)
    }
}

impl VdpRegisterPipeline {
    pub(crate) fn display_height(&self, region: Region) -> usize {
        display_height_byte(self.registers()[usize::from(MODE_REGISTER)], region)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpHBlankEdge {
    pub at: MasterTicks,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpH32Timing {
    epoch: MasterTicks,
}

impl VdpH32Timing {
    pub fn at_counter_zero(epoch: MasterTicks) -> Self {
        Self { epoch }
    }

    /// Internal nine-bit counter, not the guest HV port.
    pub fn h_counter_at(&self, at: MasterTicks) -> Result<u16, VdpTimingError> {
        let elapsed = self.elapsed(at)?;
        let dot = (elapsed % H32_LINE_TICKS) / H32_DOT_TICKS;
        let counter = if dot < H32_COUNTER_LOW_COUNT {
            dot
        } else {
            dot - H32_COUNTER_LOW_COUNT + H32_COUNTER_HIGH_START
        };
        Ok(counter as u16)
    }

    pub fn hblank_flag_at(&self, at: MasterTicks) -> Result<bool, VdpTimingError> {
        let counter = self.h_counter_at(at)?;
        Ok(!(H32_HBLANK_CLEAR_COUNTER..H32_HBLANK_SET_COUNTER).contains(&counter))
    }

    pub fn next_hblank_edge(&self, after: MasterTicks) -> Result<VdpHBlankEdge, VdpTimingError> {
        let phase = self.elapsed(after)? % H32_LINE_TICKS;
        let clear = u64::from(H32_HBLANK_CLEAR_COUNTER) * H32_DOT_TICKS;
        let set = u64::from(H32_HBLANK_SET_COUNTER) * H32_DOT_TICKS;
        let (step, active) = if phase < clear {
            (clear - phase, false)
        } else if phase < set {
            (set - phase, true)
        } else {
            (H32_LINE_TICKS - phase + clear, false)
        };
        let at = after
            .get()
            .checked_add(step)
            .ok_or(VdpTimingError::ClockOverflow)?;
        Ok(VdpHBlankEdge {
            at: MasterTicks::new(at),
            active,
        })
    }

    pub fn next_nonactive_write_slot(
        &self,
        after: MasterTicks,
    ) -> Result<MasterTicks, VdpTimingError> {
        let phase = self.elapsed(after)? % H32_MEMORY_SLOT_TICKS;
        let step = if phase < H32_DOT_TICKS {
            H32_DOT_TICKS - phase
        } else {
            H32_MEMORY_SLOT_TICKS + H32_DOT_TICKS - phase
        };
        let mut next = after
            .get()
            .checked_add(step)
            .ok_or(VdpTimingError::ClockOverflow)?;
        while H32_NONACTIVE_REFRESH.contains(&self.h_counter_at(MasterTicks::new(next))?) {
            next = next
                .checked_add(H32_MEMORY_SLOT_TICKS)
                .ok_or(VdpTimingError::ClockOverflow)?;
        }
        Ok(MasterTicks::new(next))
    }

    pub fn next_active_write_slot(
        &self,
        after: MasterTicks,
    ) -> Result<MasterTicks, VdpTimingError> {
        let phase = self.elapsed(after)? % H32_LINE_TICKS;
        let offset = H32_ACTIVE_WRITE_SLOTS
            .iter()
            .map(|&counter| {
                let counter = u64::from(counter);
                let dot = if counter < H32_COUNTER_LOW_COUNT {
                    counter
                } else {
                    counter - H32_COUNTER_HIGH_START + H32_COUNTER_LOW_COUNT
                };
                dot * H32_DOT_TICKS
            })
            .find(|&offset| offset > phase);
        let step = offset.map_or(
            H32_LINE_TICKS - phase + u64::from(H32_ACTIVE_WRITE_SLOTS[0]) * H32_DOT_TICKS,
            |offset| offset - phase,
        );
        after
            .get()
            .checked_add(step)
            .map(MasterTicks::new)
            .ok_or(VdpTimingError::ClockOverflow)
    }

    pub fn is_active_write_slot(&self, at: MasterTicks) -> Result<bool, VdpTimingError> {
        Ok(self.elapsed(at)? % H32_DOT_TICKS == 0
            && H32_ACTIVE_WRITE_SLOTS.contains(&self.h_counter_at(at)?))
    }

    fn elapsed(&self, at: MasterTicks) -> Result<u64, VdpTimingError> {
        at.get()
            .checked_sub(self.epoch.get())
            .ok_or(VdpTimingError::BeforeEpoch {
                epoch: self.epoch,
                requested: at,
            })
    }
}

fn display_height_byte(value: Option<u8>, region: Region) -> usize {
    if region == Region::Pal && value.is_some_and(|value| value & V30_ENABLE != 0) {
        V30_VBLANK_SET_LINE as usize
    } else {
        V28_VBLANK_SET_LINE as usize
    }
}

fn validate_mode_byte(
    value: Option<u8>,
    region: Region,
    require_display: bool,
) -> Result<(), VdpTimingError> {
    let display = if require_display { DISPLAY_ENABLE } else { 0 };
    let value = value.ok_or(VdpTimingError::UnknownRegister(MODE_REGISTER))?;
    if value & (MODE5_ENABLE | display) != MODE5_ENABLE | display
        || (region != Region::Pal && value & V30_ENABLE != 0)
    {
        return Err(VdpTimingError::UnsupportedMode {
            index: MODE_REGISTER,
            value,
        });
    }
    Ok(())
}

impl VdpVerticalTiming {
    pub(in crate::hardware::vdp) fn sound_mode_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &SoundRegisters,
    ) -> Result<super::mode::HorizontalMode, VdpTimingError> {
        self.horizontal.elapsed(at)?;
        validate_mode_byte(registers.mode_byte(), region, false)?;
        horizontal_mode_byte(registers.size_byte())
    }
}

mod anchored;
pub(super) mod clock;
pub(super) mod cursor;
pub(super) mod history;

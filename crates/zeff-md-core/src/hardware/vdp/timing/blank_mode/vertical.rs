use super::super::clock::LineClock;
use super::*;
mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct VerticalClock {
    pub clock: LineClock,
    pub line_start: MasterTicks,
    pub h32: u16,
    pub h40: u16,
    pub prepared_mode: u8,
    pub prepared_hint: u8,
    pub prepared_at: MasterTicks,
}

impl VerticalClock {
    pub fn new(core: core::Core, mode: u8, hint: u8, now: MasterTicks) -> Self {
        Self {
            clock: LineClock::H32,
            line_start: core.line_start,
            h32: core.row,
            h40: 0,
            prepared_mode: mode,
            prepared_hint: hint,
            prepared_at: now,
        }
    }

    pub fn selected(registers: &VdpRegisterPipeline) -> Result<LineClock, VdpTimingError> {
        let value = registers.registers()[12].ok_or(VdpTimingError::UnknownRegister(12))?;
        match value & (H40_CLOCK_MASK | INTERLACE_MODE_MASK) {
            0 => Ok(LineClock::H32),
            1 => Ok(LineClock::InternalH40),
            _ => Err(VdpTimingError::UnsupportedMode { index: 12, value }),
        }
    }

    pub fn dot(self) -> u64 {
        if self.clock == LineClock::H32 {
            H32_DOT_TICKS
        } else {
            u64::from(H40_DOT_TICKS)
        }
    }

    pub fn boundary(self) -> Result<MasterTicks, VdpTimingError> {
        self.line_start
            .get()
            .checked_add(self.clock.ticks())
            .map(MasterTicks::new)
            .ok_or(VdpTimingError::ClockOverflow)
    }

    pub fn counter(self, at: MasterTicks) -> Result<u16, VdpTimingError> {
        let phase = at
            .get()
            .checked_sub(self.line_start.get())
            .ok_or(VdpTimingError::UnqualifiedDisplayHistory)?;
        if phase == self.clock.ticks() {
            Ok(0)
        } else {
            self.clock.counter(phase)
        }
    }

    pub fn finish(
        &mut self,
        at: MasterTicks,
        core: core::Core,
        registers: &VdpRegisterPipeline,
    ) -> Result<(), VdpTimingError> {
        if at != self.boundary()? {
            return Ok(());
        }
        if core.field_start == at {
            self.h32 = 0;
            self.h40 = 0;
        } else if self.clock == LineClock::H32 {
            self.h32 = self
                .h32
                .checked_add(1)
                .ok_or(VdpTimingError::ClockOverflow)?;
        } else {
            self.h40 = self
                .h40
                .checked_add(1)
                .ok_or(VdpTimingError::ClockOverflow)?;
        }
        self.line_start = at;
        self.clock = Self::selected(registers)?;
        Ok(())
    }
}

impl BlankHistory {
    pub fn status(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<u16, VdpTimingError> {
        let word = self.core.status(region, registers) & !HBLANK_STATUS;
        Ok(word
            | if self.hblank(self.now, registers.registers()[1].unwrap())? {
                HBLANK_STATUS
            } else {
                0
            })
    }
    pub fn next_vertical_blank(
        mut self,
        after: MasterTicks,
        region: Region,
        mut regs: VdpRegisterPipeline,
    ) -> Result<VdpVBlankEdge, VdpTimingError> {
        self = self.advance_projection(after, region, &mut regs)?;
        if regs.registers()[1].unwrap() & DISPLAY_ENABLE == 0
            && !regs
                .pending()
                .is_some_and(|w| w.index == MODE_REGISTER && w.value & DISPLAY_ENABLE != 0)
        {
            return Err(VdpTimingError::ClockOverflow);
        }
        let blank = |b: &Self, r: &VdpRegisterPipeline| {
            r.registers()[1].unwrap() & DISPLAY_ENABLE == 0 || !(b.core.active || b.core.last)
        };
        let old = blank(&self, &regs);
        loop {
            let at = self.next_event(&regs)?;
            self = self.advance_projection(at, region, &mut regs)?;
            if blank(&self, &regs) != old {
                return Ok(VdpVBlankEdge { at, active: !old });
            }
            if regs.registers()[1].unwrap() & DISPLAY_ENABLE == 0 && regs.pending().is_none() {
                return Err(VdpTimingError::ClockOverflow);
            }
        }
    }
    pub fn authoritative(&self) -> bool {
        self.active || self.vertical.is_some()
    }

    pub fn vertical_supported(&self, registers: &VdpRegisterPipeline, region: Region) -> bool {
        region != Region::Pal
            && !self.active
            && (self.vertical.is_some() || self.eligible && registers.registers()[12] == Some(0))
            && registers.registers()[0].is_some_and(|v| v & 1 == 0)
            && registers.registers()[1]
                .is_some_and(|v| v & MODE5_ENABLE != 0 && v & VRAM_128K_ENABLE == 0)
            && registers.registers()[10].is_some()
            && VerticalClock::selected(registers).is_ok()
    }

    pub fn activates_vertical(
        &self,
        registers: &VdpRegisterPipeline,
        at: MasterTicks,
        region: Region,
    ) -> bool {
        self.vertical.is_none()
            && self.vertical_supported(registers, region)
            && registers.pending().is_some_and(|w| {
                w.publish_at == at
                    && w.index == MODE_REGISTER
                    && w.value & (MODE5_ENABLE | V30_ENABLE | VRAM_128K_ENABLE)
                        == MODE5_ENABLE | V30_ENABLE
            })
    }

    pub fn hblank(&self, at: MasterTicks, mode: u8) -> Result<bool, VdpTimingError> {
        if let Some(clock) = self.vertical {
            clock.clock.blank(
                at.get()
                    .checked_sub(clock.line_start.get())
                    .ok_or(VdpTimingError::UnqualifiedDisplayHistory)?,
            )
        } else {
            Ok(self.core.hblank(at, mode))
        }
    }

    pub fn slot(&self, external: bool, mode: u8) -> Result<bool, VdpTimingError> {
        let (clock, start) = self
            .vertical
            .map_or((LineClock::H32, self.core.line_start), |v| {
                (v.clock, v.line_start)
            });
        clock.is_slot(
            self.now.get() - start.get(),
            !external && mode & DISPLAY_ENABLE != 0 && (self.core.active || self.core.last),
        )
    }

    pub fn projected_slot(
        mut self,
        after: MasterTicks,
        region: Region,
        mut regs: VdpRegisterPipeline,
        external: bool,
    ) -> Result<MasterTicks, VdpTimingError> {
        self = self.advance_projection(after, region, &mut regs)?;
        loop {
            let at = self.next_event(&regs)?;
            if self.vertical.is_some()
                && let Some(port) = self.port.filter(|p| at > p.end)
            {
                let unknown = port
                    .end
                    .get()
                    .checked_add(1)
                    .map(MasterTicks::new)
                    .ok_or(VdpTimingError::ClockOverflow)?;
                return Err(VdpTimingError::UnqualifiedMemoryInterval(unknown));
            }
            self = self.advance_projection(at, region, &mut regs)?;
            if self.slot(external, regs.registers()[1].unwrap())? {
                return Ok(at);
            }
        }
    }

    pub fn vertical_position(self) -> Result<FieldPosition, VdpTimingError> {
        let clock = self
            .vertical
            .ok_or(VdpTimingError::UnqualifiedDisplayHistory)?;
        Ok(FieldPosition {
            id: self.core.field_id,
            start: self.core.field_start,
            lines: 512,
            line: self.core.row,
            line_start: clock.line_start,
            line_ticks: clock.clock.ticks(),
            projected_end: None,
            mode: InterlaceMode::Progressive,
            odd: false,
            active_mode: InterlaceMode::Progressive,
            active_odd: false,
        })
    }
}

impl VdpVerticalTiming {
    pub(in crate::hardware::vdp) fn vertical_source(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Option<BlankHistory> {
        self.blank.filter(|b| {
            b.vertical.is_some()
                || registers
                    .pending()
                    .is_some_and(|w| b.activates_vertical(registers, w.publish_at, region))
        })
    }

    pub(in crate::hardware::vdp) fn vertical_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<Option<(BlankHistory, VdpRegisterPipeline)>, VdpTimingError> {
        self.vertical_source(region, registers)
            .map(|b| {
                let mut regs = *registers;
                b.advance_projection(at, region, &mut regs)
                    .map(|b| (b, regs))
            })
            .transpose()
    }
}

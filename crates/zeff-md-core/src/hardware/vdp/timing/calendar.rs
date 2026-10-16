use super::*;

impl VdpVerticalTiming {
    pub(in crate::hardware::vdp) fn is_external_fetch_slot(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpTimingError> {
        self.elapsed_in_mode(at, region, registers, false)?;
        self.slot_at(at, false, Self::horizontal_mode(registers)?)
    }
    pub(in crate::hardware::vdp) fn next_external_fetch_slot(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpTimingError> {
        self.elapsed_in_mode(after, region, registers, false)?;
        self.next_slot(after, false, Self::horizontal_mode(registers)?)
    }
    /// Requires stable DE and mode; excludes first/last preparation cycles.
    pub fn next_memory_slot(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpTimingError> {
        let active = self.memory_active_at(after, region, registers)?;
        let next = self.next_slot(after, active, Self::horizontal_mode(registers)?)?;
        self.memory_active_at(next, region, registers)?;
        Ok(next)
    }

    pub fn is_memory_slot(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpTimingError> {
        self.slot_at(
            at,
            self.memory_active_at(at, region, registers)?,
            Self::horizontal_mode(registers)?,
        )
    }

    pub(in crate::hardware::vdp) fn next_machine_memory_slot(
        &self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpTimingError> {
        let active = self.machine_memory_active_at(after, region, registers)?;
        let mode = Self::horizontal_mode(registers)?;
        let next = self.next_slot(after, active, mode)?;
        if self.machine_memory_active_at(next, region, registers)? == active {
            return Ok(next);
        }
        let elapsed = self.horizontal.elapsed(after)?;
        let boundary = after
            .get()
            .checked_add(H32_LINE_TICKS - elapsed % H32_LINE_TICKS)
            .ok_or(VdpTimingError::ClockOverflow)?;
        let boundary = MasterTicks::new(boundary);
        let active = self.machine_memory_active_at(boundary, region, registers)?;
        if self.slot_at(boundary, active, mode)? {
            Ok(boundary)
        } else {
            self.next_slot(boundary, active, mode)
        }
    }

    pub(in crate::hardware::vdp) fn is_machine_memory_slot(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpTimingError> {
        self.slot_at(
            at,
            self.machine_memory_active_at(at, region, registers)?,
            Self::horizontal_mode(registers)?,
        )
    }

    fn next_slot(
        &self,
        after: MasterTicks,
        active: bool,
        mode: super::super::mode::HorizontalMode,
    ) -> Result<MasterTicks, VdpTimingError> {
        if mode == super::super::mode::HorizontalMode::H32 {
            return if active {
                self.horizontal.next_active_write_slot(after)
            } else {
                self.horizontal.next_nonactive_write_slot(after)
            };
        }
        let phase = self.horizontal.elapsed(after)? % H32_LINE_TICKS;
        let slots = super::super::mode::HorizontalMode::h40_slots(active);
        let offset = slots
            .iter()
            .copied()
            .find(|&offset| u64::from(offset) > phase);
        let step = offset.map_or(H32_LINE_TICKS - phase + u64::from(slots[0]), |offset| {
            u64::from(offset) - phase
        });
        after
            .get()
            .checked_add(step)
            .map(MasterTicks::new)
            .ok_or(VdpTimingError::ClockOverflow)
    }

    fn slot_at(
        &self,
        at: MasterTicks,
        active: bool,
        mode: super::super::mode::HorizontalMode,
    ) -> Result<bool, VdpTimingError> {
        if mode == super::super::mode::HorizontalMode::H32 {
            return if active {
                self.horizontal.is_active_write_slot(at)
            } else {
                self.is_write_slot(at)
            };
        }
        let phase = (self.horizontal.elapsed(at)? % H32_LINE_TICKS) as u16;
        Ok(super::super::mode::HorizontalMode::h40_slots(active)
            .binary_search(&phase)
            .is_ok())
    }

    fn machine_memory_active_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpTimingError> {
        self.elapsed_in_mode(at, region, registers, false)?;
        let position = self.field_position_at(at, region, registers)?;
        let value = registers.registers()[usize::from(MODE_REGISTER)].unwrap();
        if value & VRAM_128K_ENABLE != 0 {
            return Err(VdpTimingError::UnsupportedMode {
                index: MODE_REGISTER,
                value,
            });
        }
        Ok(value & DISPLAY_ENABLE != 0
            && u64::from(position.line) < registers.display_height(region) as u64)
    }

    fn memory_active_at(
        &self,
        at: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<bool, VdpTimingError> {
        self.elapsed_in_mode(at, region, registers, false)?;
        let position = self.field_position_at(at, region, registers)?;
        let mode = registers.registers()[usize::from(MODE_REGISTER)].unwrap();
        if mode & VRAM_128K_ENABLE != 0 {
            return Err(VdpTimingError::UnsupportedMode {
                index: MODE_REGISTER,
                value: mode,
            });
        }
        if mode & DISPLAY_ENABLE == 0 {
            return Ok(false);
        }
        let cycle = u64::from(position.line);
        let height = registers.display_height(region) as u64;
        if cycle < height - ACTIVE_MEMORY_MARGIN {
            Ok(true)
        } else if (height + NONACTIVE_MEMORY_MARGIN..position.lines - FINAL_MEMORY_MARGIN)
            .contains(&cycle)
        {
            Ok(false)
        } else {
            Err(VdpTimingError::UnqualifiedMemoryInterval(at))
        }
    }
}

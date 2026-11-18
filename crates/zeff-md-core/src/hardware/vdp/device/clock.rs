use super::*;
use crate::hardware::vdp::VdpInterruptError;

impl State {
    pub(super) fn changes_vertical_mode(&self, index: u8, value: u8) -> bool {
        index == MODE_REGISTER
            && self.interrupts.m5_vertical()
            && value & (MODE5_ENABLE | VRAM_128K_ENABLE) == MODE5_ENABLE
            && self.registers.registers()[1].is_some_and(|old| {
                (old ^ value) & MEMORY_MODE_MASK & !(DISPLAY_ENABLE | V30_ENABLE) == 0
            })
    }
    pub(super) fn changes_progressive_clock(&self, index: u8, value: u8) -> bool {
        if self.interrupts.m5_vertical() {
            return index == DISPLAY_SIZE_REGISTER
                && matches!(value & (H40_CLOCK_MASK | INTERLACE_MODE_MASK), 0 | 1)
                && self.registers.registers()[12].is_some_and(|old| {
                    matches!(old & (H40_CLOCK_MASK | INTERLACE_MODE_MASK), 0 | 1)
                        && (old ^ value) & H40_CLOCK_MASK != 0
                });
        }
        index == DISPLAY_SIZE_REGISTER
            && matches!(
                (
                    self.registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)],
                    value
                ),
                (Some(0), H40_CLOCK_MASK) | (Some(H40_CLOCK_MASK), 0)
            )
    }

    pub(super) fn validate_clock_write(
        &self,
        write: VdpRegisterWrite,
        region: Region,
    ) -> Result<(), VdpScheduleError> {
        if self.interrupts.m5_vertical()
            && write.index == DISPLAY_SIZE_REGISTER
            && !matches!(write.value & (H40_CLOCK_MASK | INTERLACE_MODE_MASK), 0 | 1)
        {
            return Err(VdpRegisterError::UnsupportedModeChange {
                index: write.index,
                value: write.value,
            }
            .into());
        }
        if write.index == DISPLAY_SIZE_REGISTER
            && (write.value & H40_CLOCK_MASK == 1
                || self.registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
                    .is_some_and(|old| old & H40_CLOCK_MASK == 1)
                || self.interrupts.has_timing_history())
        {
            let position = self
                .interrupts
                .field_position(region, &self.registers)
                .map_err(VdpInterruptError::from)?;
            if write.value & INTERLACE_MODE_MASK != 0
                || self.registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
                    .is_some_and(|old| old & INTERLACE_MODE_MASK != 0)
                || position.mode.interlaced()
                || position.active_mode.interlaced()
            {
                #[cfg(feature = "first-failure-diagnostic")]
                self.record_clock_rejection(
                    crate::diagnostic::RejectionSite::ClockInterlace,
                    write,
                );
                return Err(VdpRegisterError::UnsupportedModeChange {
                    index: write.index,
                    value: write.value,
                }
                .into());
            }
            return Ok(());
        }
        let normal_clock = write.index == DISPLAY_SIZE_REGISTER
            && self.registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)].is_some_and(|old| {
                (old ^ write.value) & H40_CLOCK_MASK != 0
                    && (old & INTERLACE_MODE_MASK == 2
                        || write.value & INTERLACE_MODE_MASK == 2
                        || self.interrupts.has_normal_interlace_history())
            });
        if !self.changes_progressive_clock(write.index, write.value) && !normal_clock {
            return Ok(());
        }
        let published = self.registers.published_state().1;
        let current = self
            .interrupts
            .field_position(region, &published)
            .map_err(VdpInterruptError::from)?;
        let position = self
            .interrupts
            .field_position_at(write.publish_at, region, &published)
            .map_err(VdpInterruptError::from)?;
        let phase = (write.publish_at.get() - position.start.get()) % H32_LINE_TICKS;
        // Keep both counter edges on the same side of publication.
        if current.mode.interlaced()
            || current.active_mode.interlaced()
            || position.mode.interlaced()
            || position.active_mode.interlaced()
            || (H40_VCOUNTER_TICKS..H32_VCOUNTER_INCREMENT * H32_DOT_TICKS).contains(&phase)
        {
            #[cfg(feature = "first-failure-diagnostic")]
            self.record_clock_rejection(
                crate::diagnostic::RejectionSite::ClockPublicationWindow,
                write,
            );
            return Err(VdpRegisterError::UnsupportedModeChange {
                index: write.index,
                value: write.value,
            }
            .into());
        }
        Ok(())
    }

    pub(super) fn pending_fifo_mode_change(&self, region: Region) -> Option<VdpRegisterWrite> {
        self.registers.pending().filter(|write| {
            self.read_phase.is_none()
                && self.dma.is_none()
                && !self.fifo.is_empty()
                && (self
                    .registers
                    .changes_display_enable(write.index, write.value)
                    || self.changes_vertical_mode(write.index, write.value)
                    || (self.changes_progressive_clock(write.index, write.value)
                        && self.validate_clock_write(*write, region).is_ok()))
        })
    }
}

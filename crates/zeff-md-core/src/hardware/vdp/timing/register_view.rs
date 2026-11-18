use super::*;
use crate::hardware::vdp::VdpRegisterError;

pub(in crate::hardware::vdp) trait CalendarRegisters: Copy {
    fn calendar_now(&self) -> MasterTicks;
    fn pending_internal_clock(&self) -> bool;
    fn mode_byte(&self) -> Option<u8>;
    fn size_byte(&self) -> Option<u8>;
    fn pending_publication(&self) -> Option<MasterTicks>;
    fn advance_calendar_to(&mut self, at: MasterTicks) -> Result<(), VdpRegisterError>;
}

impl CalendarRegisters for VdpRegisterPipeline {
    fn calendar_now(&self) -> MasterTicks {
        self.now()
    }

    fn pending_internal_clock(&self) -> bool {
        self.pending().is_some_and(|write| {
            write.index == DISPLAY_SIZE_REGISTER
                && write.value & (H40_CLOCK_MASK | INTERLACE_MODE_MASK) == 1
                && self
                    .size_byte()
                    .is_some_and(|old| old & INTERLACE_MODE_MASK == 0)
                && self
                    .mode_byte()
                    .is_some_and(|mode| mode & MODE5_ENABLE != 0)
        })
    }

    fn mode_byte(&self) -> Option<u8> {
        self.registers()[usize::from(MODE_REGISTER)]
    }

    fn size_byte(&self) -> Option<u8> {
        self.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
    }

    fn pending_publication(&self) -> Option<MasterTicks> {
        self.pending().map(|write| write.publish_at)
    }

    fn advance_calendar_to(&mut self, at: MasterTicks) -> Result<(), VdpRegisterError> {
        self.advance_to(at).map(|_| ())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct SoundRegisters {
    now: MasterTicks,
    pending_at: MasterTicks,
    r1: Option<u8>,
    r12: Option<u8>,
    pending_index: Option<u8>,
    pending_value: u8,
}

impl SoundRegisters {
    pub(in crate::hardware::vdp) fn capture(registers: &VdpRegisterPipeline) -> Self {
        let pending = registers.pending();
        Self {
            now: registers.now(),
            pending_at: pending.map_or(MasterTicks::ZERO, |write| write.publish_at),
            r1: registers.mode_byte(),
            r12: registers.size_byte(),
            pending_index: pending.map(|write| write.index),
            pending_value: pending.map_or(0, |write| write.value),
        }
    }

    pub(in crate::hardware::vdp) fn display_height(&self, region: Region) -> usize {
        display_height_byte(self.r1, region)
    }
}

impl CalendarRegisters for SoundRegisters {
    fn calendar_now(&self) -> MasterTicks {
        self.now
    }

    fn pending_internal_clock(&self) -> bool {
        self.pending_index == Some(DISPLAY_SIZE_REGISTER)
            && self.pending_value & (H40_CLOCK_MASK | INTERLACE_MODE_MASK) == 1
            && self.r12.is_some_and(|old| old & INTERLACE_MODE_MASK == 0)
            && self.r1.is_some_and(|mode| mode & MODE5_ENABLE != 0)
    }

    fn mode_byte(&self) -> Option<u8> {
        self.r1
    }

    fn size_byte(&self) -> Option<u8> {
        self.r12
    }

    fn pending_publication(&self) -> Option<MasterTicks> {
        self.pending_index.map(|_| self.pending_at)
    }

    fn advance_calendar_to(&mut self, at: MasterTicks) -> Result<(), VdpRegisterError> {
        if at < self.now {
            return Err(VdpRegisterError::TimeReversal {
                current: self.now,
                requested: at,
            });
        }
        self.now = at;
        if self.pending_index.is_some() && self.pending_at <= at {
            match self.pending_index.take().unwrap() {
                MODE_REGISTER => self.r1 = Some(self.pending_value),
                DISPLAY_SIZE_REGISTER => self.r12 = Some(self.pending_value),
                _ => {}
            }
        }
        Ok(())
    }
}

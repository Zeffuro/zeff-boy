use super::*;
use crate::hardware::vdp::mode::HorizontalMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) enum LineClock {
    H32,
    ExternalH40,
    InternalH40,
}

pub(in crate::hardware::vdp) const INTERNAL_LINE_TICKS: u64 =
    H40_LINE_DOTS as u64 * H40_DOT_TICKS as u64;
pub(in crate::hardware::vdp) const LINE_PHASE_TICKS: u64 = H32_LINE_TICKS - INTERNAL_LINE_TICKS;
const INTERNAL_ACTIVE: [u16; H40_ACTIVE_WRITE_SLOTS.len()] = internal_slots(true);
const INTERNAL_IDLE: [u16; H40_NONACTIVE_SLOT_COUNT] = internal_slots(false);

const fn internal_slots<const N: usize>(active: bool) -> [u16; N] {
    let mut slots = [0; N];
    let mut dot = 0;
    let mut count = 0;
    while dot < H40_LINE_DOTS {
        let counter = horizontal::h40_counter(dot);
        let mut selected = !active && counter & 1 == 0;
        let values: &[u16] = if active {
            &H40_ACTIVE_WRITE_SLOTS
        } else {
            &H40_NONACTIVE_REFRESH
        };
        let mut index = 0;
        while index < values.len() {
            if counter == values[index] {
                selected = active;
            }
            index += 1;
        }
        if selected {
            slots[count] = dot as u16 * H40_DOT_TICKS;
            count += 1;
        }
        dot += 1;
    }
    assert!(count == N);
    slots
}

impl LineClock {
    pub(in crate::hardware::vdp) fn scroll_fetch_phase(self) -> u64 {
        if self == Self::InternalH40 {
            u64::from(H40_HSCROLL_FETCH_COUNTER - H40_COUNTER_HIGH_START + H40_COUNTER_LOW_COUNT)
                * u64::from(H40_DOT_TICKS)
        } else {
            self.mode().scroll_fetch_phase()
        }
    }

    pub(in crate::hardware::vdp) fn from_register(value: u8) -> Result<Self, VdpTimingError> {
        let clock = match value & H40_CLOCK_MASK {
            0 => Self::H32,
            1 => Self::InternalH40,
            H40_CLOCK_MASK => Self::ExternalH40,
            _ => return Err(Self::unsupported(value)),
        };
        let interlace = InterlaceMode::from_register(value).ok_or(Self::unsupported(value))?;
        if clock == Self::InternalH40 && interlace.interlaced() {
            return Err(Self::unsupported(value));
        }
        Ok(clock)
    }

    fn unsupported(value: u8) -> VdpTimingError {
        VdpTimingError::UnsupportedMode {
            index: DISPLAY_SIZE_REGISTER,
            value,
        }
    }

    pub(in crate::hardware::vdp) fn ticks(self) -> u64 {
        if self == Self::InternalH40 {
            INTERNAL_LINE_TICKS
        } else {
            H32_LINE_TICKS
        }
    }

    pub(in crate::hardware::vdp) fn mode(self) -> HorizontalMode {
        if self == Self::H32 {
            HorizontalMode::H32
        } else {
            HorizontalMode::H40
        }
    }

    fn check(self, phase: u64) -> Result<(), VdpTimingError> {
        if phase < self.ticks() {
            Ok(())
        } else {
            Err(VdpTimingError::UnqualifiedDisplayHistory)
        }
    }

    pub(in crate::hardware::vdp) fn counter(self, phase: u64) -> Result<u16, VdpTimingError> {
        self.check(phase)?;
        Ok(if self == Self::InternalH40 {
            horizontal::h40_counter((phase / u64::from(H40_DOT_TICKS)) as usize)
        } else {
            self.mode().counter(phase)
        })
    }

    pub(in crate::hardware::vdp) fn blank(self, phase: u64) -> Result<bool, VdpTimingError> {
        self.check(phase)?;
        Ok(self.mode().blank(phase))
    }

    pub(in crate::hardware::vdp) fn vertical_clock(self) -> u64 {
        self.mode().vertical_clock()
    }
    pub(in crate::hardware::vdp) fn vertical_interrupt(self) -> u64 {
        self.mode().vertical_interrupt()
    }

    fn h40_slots(self, active: bool) -> &'static [u16] {
        if self == Self::InternalH40 {
            if active {
                &INTERNAL_ACTIVE
            } else {
                &INTERNAL_IDLE
            }
        } else {
            HorizontalMode::h40_slots(active)
        }
    }

    pub(in crate::hardware::vdp) fn is_slot(
        self,
        phase: u64,
        active: bool,
    ) -> Result<bool, VdpTimingError> {
        self.check(phase)?;
        if self != Self::H32 {
            return Ok(self
                .h40_slots(active)
                .binary_search(&(phase as u16))
                .is_ok());
        }
        let counter = self.counter(phase)?;
        Ok(if active {
            phase.is_multiple_of(H32_DOT_TICKS) && H32_ACTIVE_WRITE_SLOTS.contains(&counter)
        } else {
            phase % H32_MEMORY_SLOT_TICKS == H32_DOT_TICKS
                && !H32_NONACTIVE_REFRESH.contains(&counter)
        })
    }

    pub(in crate::hardware::vdp) fn next_slot(
        self,
        phase: u64,
        active: bool,
    ) -> Result<Option<u64>, VdpTimingError> {
        self.check(phase)?;
        if self != Self::H32 {
            let slots = self.h40_slots(active);
            return Ok(slots
                .get(slots.partition_point(|&tick| u64::from(tick) <= phase))
                .map(|&tick| u64::from(tick)));
        }
        let timing = VdpH32Timing::at_counter_zero(MasterTicks::ZERO);
        let next = if active {
            timing.next_active_write_slot(MasterTicks::new(phase))?
        } else {
            timing.next_nonactive_write_slot(MasterTicks::new(phase))?
        };
        Ok((next.get() < self.ticks()).then_some(next.get()))
    }
}

#[cfg(test)]
mod tests;

use super::*;
use crate::hardware::vdp::mode::HorizontalMode;

const H40_OFFSETS: [u16; H40_LINE_DOTS + 1] = h40_offsets();
const H40_NONACTIVE_OFFSETS: [u16; H40_NONACTIVE_SLOT_COUNT] = h40_nonactive_offsets();
const H40_ACTIVE_OFFSETS: [u16; H40_ACTIVE_WRITE_SLOTS.len()] = h40_active_offsets();

const fn h40_counter(dot: usize) -> u16 {
    if dot < H40_COUNTER_LOW_COUNT as usize {
        dot as u16
    } else {
        dot as u16 - H40_COUNTER_LOW_COUNT + H40_COUNTER_HIGH_START
    }
}

const fn h40_offsets() -> [u16; H40_LINE_DOTS + 1] {
    let mut offsets = [0; H40_LINE_DOTS + 1];
    let mut dot = 0;
    while dot < H40_LINE_DOTS {
        let counter = h40_counter(dot);
        let duration = if counter >= H40_SYNC_START_COUNTER && counter <= H40_SYNC_END_COUNTER {
            if counter == H40_SYNC_SHORTEST_COUNTER {
                H40_DOT_TICKS
            } else if counter == H40_SYNC_SHORT_COUNTERS[0]
                || counter == H40_SYNC_SHORT_COUNTERS[1]
                || counter == H40_SYNC_SHORT_COUNTERS[2]
                || counter == H40_SYNC_SHORT_COUNTERS[3]
            {
                H40_SYNC_DOT_TICKS - 1
            } else {
                H40_SYNC_DOT_TICKS
            }
        } else {
            H40_DOT_TICKS
        };
        offsets[dot + 1] = offsets[dot] + duration;
        dot += 1;
    }
    assert!(offsets[H40_LINE_DOTS] as u64 == H32_LINE_TICKS);
    offsets
}

const fn h40_nonactive_offsets() -> [u16; H40_NONACTIVE_SLOT_COUNT] {
    let mut slots = [0; H40_NONACTIVE_SLOT_COUNT];
    let mut dot = 0;
    let mut count = 0;
    while dot < H40_LINE_DOTS {
        let counter = h40_counter(dot);
        let mut refresh = false;
        let mut index = 0;
        while index < H40_NONACTIVE_REFRESH.len() {
            refresh |= counter == H40_NONACTIVE_REFRESH[index];
            index += 1;
        }
        if counter & 1 == 0 && !refresh {
            slots[count] = H40_OFFSETS[dot];
            count += 1;
        }
        dot += 1;
    }
    assert!(count == H40_NONACTIVE_SLOT_COUNT);
    slots
}

const fn h40_active_offsets() -> [u16; H40_ACTIVE_WRITE_SLOTS.len()] {
    let mut slots = [0; H40_ACTIVE_WRITE_SLOTS.len()];
    let mut index = 0;
    while index < slots.len() {
        let counter = H40_ACTIVE_WRITE_SLOTS[index];
        let dot = if counter < H40_COUNTER_LOW_COUNT {
            counter
        } else {
            counter - H40_COUNTER_HIGH_START + H40_COUNTER_LOW_COUNT
        };
        slots[index] = H40_OFFSETS[dot as usize];
        index += 1;
    }
    slots
}

impl HorizontalMode {
    pub(super) fn counter(self, phase: u64) -> u16 {
        match self {
            Self::H32 => {
                let dot = phase / H32_DOT_TICKS;
                if dot < H32_COUNTER_LOW_COUNT {
                    dot as u16
                } else {
                    (dot - H32_COUNTER_LOW_COUNT + H32_COUNTER_HIGH_START) as u16
                }
            }
            Self::H40 => {
                h40_counter(H40_OFFSETS.partition_point(|&tick| u64::from(tick) <= phase) - 1)
            }
        }
    }

    pub(super) fn vertical_clock(self) -> u64 {
        match self {
            Self::H32 => H32_VCOUNTER_INCREMENT * H32_DOT_TICKS,
            Self::H40 => H40_VCOUNTER_TICKS,
        }
    }

    pub(in crate::hardware::vdp) fn vertical_interrupt(self) -> u64 {
        match self {
            Self::H32 => H32_VINT_COUNTER * H32_DOT_TICKS,
            Self::H40 => u64::from(H40_DOT_TICKS),
        }
    }

    pub(super) fn blank(self, phase: u64) -> bool {
        let (clear, set) = match self {
            Self::H32 => (
                u64::from(H32_HBLANK_CLEAR_COUNTER) * H32_DOT_TICKS,
                u64::from(H32_HBLANK_SET_COUNTER) * H32_DOT_TICKS,
            ),
            Self::H40 => (H40_HBLANK_CLEAR_TICKS, H40_HBLANK_SET_TICKS),
        };
        !(clear..set).contains(&phase)
    }

    pub(super) fn h40_slots(active: bool) -> &'static [u16] {
        if active {
            &H40_ACTIVE_OFFSETS
        } else {
            &H40_NONACTIVE_OFFSETS
        }
    }
}

impl VdpVerticalTiming {
    pub(super) fn horizontal_mode(
        registers: &VdpRegisterPipeline,
    ) -> Result<HorizontalMode, VdpTimingError> {
        let value = registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
            .ok_or(VdpTimingError::UnknownRegister(DISPLAY_SIZE_REGISTER))?;
        if matches!(value & INTERLACE_MODE_MASK, 0 | INTERLACE_MODE_MASK)
            && let Some(mode) = HorizontalMode::from_register(value)
        {
            Ok(mode)
        } else {
            Err(VdpTimingError::UnsupportedMode {
                index: DISPLAY_SIZE_REGISTER,
                value,
            })
        }
    }
}

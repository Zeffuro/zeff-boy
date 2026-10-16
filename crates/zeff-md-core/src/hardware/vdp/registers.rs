use super::constants::*;
use zeff_emu_common::time::MasterTicks;
mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpRegisterWrite {
    pub index: u8,
    pub value: u8,
    pub publish_at: MasterTicks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpRegisterError {
    UnsupportedRegister(u8),
    UnsupportedModeChange {
        index: u8,
        value: u8,
    },
    Busy {
        publish_at: MasterTicks,
    },
    CoincidentClockEdge,
    TimeReversal {
        current: MasterTicks,
        requested: MasterTicks,
    },
    ClockOverflow,
}

/// Publishes mode-5 payloads on the known internal phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpRegisterPipeline {
    epoch: MasterTicks,
    now: MasterTicks,
    pending: Option<VdpRegisterWrite>,
    registers: [Option<u8>; REGISTER_COUNT as usize],
}

impl VdpRegisterPipeline {
    pub(super) fn publish_dma_counters(&mut self, dma: super::VdpDmaRegisters) {
        let [low, high] = dma.length().to_le_bytes();
        self.registers[usize::from(DMA_LENGTH_LOW_REGISTER)] = Some(low);
        self.registers[usize::from(DMA_LENGTH_HIGH_REGISTER)] = Some(high);
        let [low, high] = dma.source().to_le_bytes();
        self.registers[usize::from(DMA_SOURCE_LOW_REGISTER)] = Some(low);
        self.registers[usize::from(DMA_SOURCE_HIGH_REGISTER)] = Some(high);
    }

    pub(super) fn bootstrap_mode5() -> Self {
        let mut registers = Self::at_h32_edge(MasterTicks::ZERO);
        registers.registers.fill(Some(0));
        registers.registers[usize::from(MODE_REGISTER)] = Some(MODE5_ENABLE);
        registers
    }

    pub(super) fn published_state(&self) -> (MasterTicks, Self) {
        let mut published = *self;
        published.pending = None;
        (self.epoch, published)
    }

    pub fn at_h32_edge(edge: MasterTicks) -> Self {
        Self {
            epoch: edge,
            now: edge,
            pending: None,
            registers: [None; REGISTER_COUNT as usize],
        }
    }

    pub fn now(&self) -> MasterTicks {
        self.now
    }

    pub fn pending(&self) -> Option<VdpRegisterWrite> {
        self.pending
    }

    pub(super) fn changes_memory_mode(&self, index: u8, value: u8) -> bool {
        match index {
            MODE_REGISTER => self.registers[usize::from(MODE_REGISTER)]
                .is_none_or(|previous| (previous ^ value) & MEMORY_MODE_MASK != 0),
            DISPLAY_SIZE_REGISTER => {
                self.registers[usize::from(DISPLAY_SIZE_REGISTER)].is_none_or(|previous| {
                    let deferred = if matches!(previous & INTERLACE_MODE_MASK, 0 | 6)
                        && matches!(value & INTERLACE_MODE_MASK, 0 | 6)
                    {
                        INTERLACE_MODE_MASK
                    } else {
                        0
                    };
                    (previous ^ value) & !(SHADOW_HIGHLIGHT_ENABLE | deferred) != 0
                })
            }
            _ => false,
        }
    }

    pub(super) fn pending_memory_mode_change(&self) -> bool {
        self.pending
            .is_some_and(|write| self.changes_memory_mode(write.index, write.value))
    }

    pub(super) fn changes_display_enable(&self, index: u8, value: u8) -> bool {
        index == MODE_REGISTER
            && self.registers[usize::from(MODE_REGISTER)]
                .is_some_and(|previous| (previous ^ value) & MEMORY_MODE_MASK == DISPLAY_ENABLE)
    }

    /// Published raw payloads for inspection; None denotes an unknown value.
    pub fn registers(&self) -> &[Option<u8>; REGISTER_COUNT as usize] {
        &self.registers
    }

    /// The published R15 setting; unknown until its first write completes.
    pub fn auto_increment(&self) -> Option<u8> {
        self.registers[usize::from(AUTO_INCREMENT_REGISTER)]
    }

    pub fn plane_a_base(&self) -> Option<u16> {
        self.table_base(
            PLANE_A_TABLE_REGISTER,
            PLANE_A_TABLE_MASK,
            PLANE_A_TABLE_SHIFT,
        )
    }

    pub fn window_base(&self) -> Option<u16> {
        let mask = if self.horizontal_mode() == Some(super::mode::HorizontalMode::H40) {
            WINDOW_TABLE_H40_MASK
        } else {
            WINDOW_TABLE_H32_MASK
        };
        self.table_base(WINDOW_TABLE_REGISTER, mask, WINDOW_TABLE_SHIFT)
    }

    pub fn plane_b_base(&self) -> Option<u16> {
        self.table_base(
            PLANE_B_TABLE_REGISTER,
            PLANE_B_TABLE_MASK,
            PLANE_B_TABLE_SHIFT,
        )
    }

    pub fn sprite_table_base(&self) -> Option<u16> {
        let mask = if self.horizontal_mode() == Some(super::mode::HorizontalMode::H40) {
            SPRITE_TABLE_H40_MASK
        } else {
            SPRITE_TABLE_H32_MASK
        };
        self.table_base(SPRITE_TABLE_REGISTER, mask, SPRITE_TABLE_SHIFT)
    }

    pub fn hscroll_table_base(&self) -> Option<u16> {
        self.table_base(
            HSCROLL_TABLE_REGISTER,
            HSCROLL_TABLE_MASK,
            HSCROLL_TABLE_SHIFT,
        )
    }

    fn table_base(&self, index: u8, mask: u8, shift: u32) -> Option<u16> {
        self.registers[usize::from(index)].map(|value| u16::from(value & mask) << shift)
    }

    pub(super) fn horizontal_mode(&self) -> Option<super::mode::HorizontalMode> {
        self.registers[usize::from(DISPLAY_SIZE_REGISTER)]
            .and_then(super::mode::HorizontalMode::from_register)
    }

    pub fn queue_register(
        &mut self,
        index: u8,
        value: u8,
    ) -> Result<VdpRegisterWrite, VdpRegisterError> {
        self.queue_register_at(index, value, false)
    }

    pub(super) fn queue_control_register(
        &mut self,
        index: u8,
        value: u8,
    ) -> Result<VdpRegisterWrite, VdpRegisterError> {
        self.queue_register_at(index, value, true)
    }

    fn queue_register_at(
        &mut self,
        index: u8,
        value: u8,
        allow_edge: bool,
    ) -> Result<VdpRegisterWrite, VdpRegisterError> {
        if index >= REGISTER_COUNT {
            return Err(VdpRegisterError::UnsupportedRegister(index));
        }
        if (index == MODE_REGISTER && value & MODE5_ENABLE == 0)
            || (index == DISPLAY_SIZE_REGISTER
                && super::mode::HorizontalMode::from_register(value).is_none())
        {
            return Err(VdpRegisterError::UnsupportedModeChange { index, value });
        }
        if let Some(write) = self.pending {
            return Err(VdpRegisterError::Busy {
                publish_at: write.publish_at,
            });
        }
        let phase = (self.now.get() - self.epoch.get()) % H32_INTERNAL_CLOCK_TICKS;
        if phase == 0 && !allow_edge {
            return Err(VdpRegisterError::CoincidentClockEdge);
        }
        let publish_at = self
            .now
            .get()
            .checked_add(H32_INTERNAL_CLOCK_TICKS - phase)
            .ok_or(VdpRegisterError::ClockOverflow)?;
        let write = VdpRegisterWrite {
            index,
            value,
            publish_at: MasterTicks::new(publish_at),
        };
        self.pending = Some(write);
        Ok(write)
    }

    /// Returns each due payload once, timestamped at its edge rather than the horizon.
    pub fn advance_to(
        &mut self,
        at: MasterTicks,
    ) -> Result<Option<VdpRegisterWrite>, VdpRegisterError> {
        if at < self.now {
            return Err(VdpRegisterError::TimeReversal {
                current: self.now,
                requested: at,
            });
        }
        self.now = at;
        if self.pending.is_some_and(|write| write.publish_at <= at) {
            let write = self.pending.take().unwrap();
            self.registers[usize::from(write.index)] = Some(write.value);
            Ok(Some(write))
        } else {
            Ok(None)
        }
    }
}

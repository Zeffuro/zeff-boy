use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SoundDma {
    reload_source: u32,
    reload_length: u32,
    cycle_accumulator: u32,
}

impl Bus {
    fn dma_source_offset(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(DMA_SOURCE_LO_PORT)],
            self.io[usize::from(DMA_SOURCE_HI_PORT)],
        ])
    }

    fn set_dma_source_offset(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(DMA_SOURCE_LO_PORT)] = lo;
        self.io[usize::from(DMA_SOURCE_HI_PORT)] = hi;
    }

    fn dma_source_segment(&self) -> u16 {
        u16::from(self.io[usize::from(DMA_SOURCE_SEGMENT_PORT)] & 0x0F)
    }

    fn set_dma_source_segment(&mut self, value: u16) {
        self.io[usize::from(DMA_SOURCE_SEGMENT_PORT)] = (value & 0x0F) as u8;
        self.io[usize::from(DMA_SOURCE_SEGMENT_HIGH_PORT)] = 0;
    }

    fn dma_destination(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(DMA_DESTINATION_LO_PORT)],
            self.io[usize::from(DMA_DESTINATION_HI_PORT)],
        ])
    }

    fn set_dma_destination(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(DMA_DESTINATION_LO_PORT)] = lo;
        self.io[usize::from(DMA_DESTINATION_HI_PORT)] = hi;
    }

    fn dma_length(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(DMA_LENGTH_LO_PORT)],
            self.io[usize::from(DMA_LENGTH_HI_PORT)],
        ])
    }

    fn set_dma_length(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(DMA_LENGTH_LO_PORT)] = lo;
        self.io[usize::from(DMA_LENGTH_HI_PORT)] = hi;
    }

    pub(super) fn run_dma_transfer(&mut self, control: u8) {
        let mut source =
            u32::from(self.dma_source_offset()) | (u32::from(self.dma_source_segment()) << 16);
        let mut destination = u32::from(self.dma_destination());
        let mut remaining = self.dma_length();
        let decrement = control & 0x40 != 0;

        if !self.gdma_source_accessible(source) {
            return;
        }

        let transfer_cycles = if remaining == 0 {
            0
        } else {
            5 + u32::from(remaining)
        };

        while remaining > 0 {
            let lo = self.peek8(source);
            let hi = self.peek8(source.wrapping_add(1));
            self.write8(destination, lo);
            self.write8(destination.wrapping_add(1), hi);
            remaining = remaining.saturating_sub(2);
            if decrement {
                source = source.wrapping_sub(2);
                destination = destination.wrapping_sub(2);
            } else {
                source = source.wrapping_add(2);
                destination = destination.wrapping_add(2);
            }
        }

        self.set_dma_source_offset(source as u16);
        self.set_dma_source_segment((source >> 16) as u16);
        self.set_dma_destination(destination as u16);
        self.set_dma_length(0);
        self.io[usize::from(DMA_CONTROL_PORT)] = control & 0x7F;
        self.step_cycles(transfer_cycles);
    }

    fn gdma_source_accessible(&self, source: u32) -> bool {
        match source & ADDRESS_MASK {
            0x00000..=0x0FFFF => true,
            0x10000..=0x1FFFF => false,
            0x20000..=0x7FFFF => true,
            0x80000..=0xFFFFF => {
                self.io[usize::from(SYSTEM_CONTROL_PORT)] & SYSTEM_CTRL1_ROM_WAIT == 0
            }
            _ => false,
        }
    }

    fn sound_dma_source(&self) -> u32 {
        u32::from(u16::from_le_bytes([
            self.io[usize::from(SOUND_DMA_SOURCE_LO_PORT)],
            self.io[usize::from(SOUND_DMA_SOURCE_HI_PORT)],
        ])) | (u32::from(self.io[usize::from(SOUND_DMA_SOURCE_SEGMENT_PORT)] & 0x0F) << 16)
    }

    fn set_sound_dma_source(&mut self, value: u32) {
        let value = value & ADDRESS_MASK;
        let [lo, hi, segment, _] = value.to_le_bytes();
        self.io[usize::from(SOUND_DMA_SOURCE_LO_PORT)] = lo;
        self.io[usize::from(SOUND_DMA_SOURCE_HI_PORT)] = hi;
        self.io[usize::from(SOUND_DMA_SOURCE_SEGMENT_PORT)] = segment & 0x0F;
        self.io[usize::from(SOUND_DMA_SOURCE_SEGMENT_HIGH_PORT)] = 0;
    }

    fn sound_dma_length(&self) -> u32 {
        u32::from(u16::from_le_bytes([
            self.io[usize::from(SOUND_DMA_LENGTH_LO_PORT)],
            self.io[usize::from(SOUND_DMA_LENGTH_HI_PORT)],
        ])) | (u32::from(self.io[usize::from(SOUND_DMA_LENGTH_SEGMENT_PORT)] & 0x0F) << 16)
    }

    fn set_sound_dma_length(&mut self, value: u32) {
        let value = value & ADDRESS_MASK;
        let [lo, hi, segment, _] = value.to_le_bytes();
        self.io[usize::from(SOUND_DMA_LENGTH_LO_PORT)] = lo;
        self.io[usize::from(SOUND_DMA_LENGTH_HI_PORT)] = hi;
        self.io[usize::from(SOUND_DMA_LENGTH_SEGMENT_PORT)] = segment & 0x0F;
        self.io[usize::from(SOUND_DMA_LENGTH_SEGMENT_HIGH_PORT)] = 0;
    }

    pub(super) fn write_sound_dma_control(&mut self, value: u8) {
        let old_control = self.io[usize::from(SOUND_DMA_CONTROL_PORT)];
        let mut control = value & SOUND_DMA_CONTROL_MASK;
        if control & SOUND_DMA_ENABLE != 0 && self.sound_dma_length() == 0 {
            control &= !SOUND_DMA_ENABLE;
        }
        self.io[usize::from(SOUND_DMA_CONTROL_PORT)] = control;

        let was_enabled = old_control & SOUND_DMA_ENABLE != 0;
        let is_enabled = control & SOUND_DMA_ENABLE != 0;
        if is_enabled && !was_enabled {
            self.sound_dma.reload_source = self.sound_dma_source();
            self.sound_dma.reload_length = self.sound_dma_length();
            self.sound_dma.cycle_accumulator = 0;
        }
        if !is_enabled {
            self.sound_dma.cycle_accumulator = 0;
        }
    }

    pub(super) fn step_sound_dma(&mut self, cycles: u32) {
        let control = self.io[usize::from(SOUND_DMA_CONTROL_PORT)];
        if control & SOUND_DMA_ENABLE == 0 {
            return;
        }

        let period = sound_dma_cycle_period(control);
        let mut available = self.sound_dma.cycle_accumulator.saturating_add(cycles);
        while available >= period {
            available -= period;
            if control & SOUND_DMA_HOLD != 0 {
                self.write_sound_dma_target(control, 0);
            } else {
                if !self.transfer_sound_dma_byte(control) {
                    available = 0;
                    break;
                }
                if self.io[usize::from(SOUND_DMA_CONTROL_PORT)] & SOUND_DMA_ENABLE == 0 {
                    available = 0;
                    break;
                }
            }
        }
        self.sound_dma.cycle_accumulator = available;
    }

    fn transfer_sound_dma_byte(&mut self, control: u8) -> bool {
        let mut length = self.sound_dma_length();
        if length == 0 {
            if control & SOUND_DMA_REPEAT == 0 || self.sound_dma.reload_length == 0 {
                self.io[usize::from(SOUND_DMA_CONTROL_PORT)] = control & !SOUND_DMA_ENABLE;
                return false;
            }
            self.set_sound_dma_source(self.sound_dma.reload_source);
            self.set_sound_dma_length(self.sound_dma.reload_length);
            length = self.sound_dma.reload_length;
        }

        let source = self.sound_dma_source();
        let value = self.peek8(source);
        self.write_sound_dma_target(control, value);
        let next_source = if control & SOUND_DMA_DECREMENT != 0 {
            source.wrapping_sub(1)
        } else {
            source.wrapping_add(1)
        };
        self.set_sound_dma_source(next_source & ADDRESS_MASK);
        let next_length = length - 1;
        self.set_sound_dma_length(next_length);
        if next_length == 0 && control & SOUND_DMA_REPEAT == 0 {
            self.io[usize::from(SOUND_DMA_CONTROL_PORT)] = control & !SOUND_DMA_ENABLE;
        }
        true
    }

    fn write_sound_dma_target(&mut self, control: u8, value: u8) {
        if control & SOUND_DMA_TARGET_HYPERVOICE != 0 {
            self.apu.write_hyper_voice_dma_sample(value);
        } else {
            self.apu.write8(SOUND_VOLUME_CHANNEL2_PORT, value);
        }
    }

    pub(crate) fn sound_dma_save_values(&self) -> (u32, u32, u32) {
        (
            self.sound_dma.reload_source,
            self.sound_dma.reload_length,
            self.sound_dma.cycle_accumulator,
        )
    }

    pub(crate) fn load_sound_dma_save_values(
        &mut self,
        reload_source: u32,
        reload_length: u32,
        cycle_accumulator: u32,
    ) {
        self.sound_dma.reload_source = reload_source & ADDRESS_MASK;
        self.sound_dma.reload_length = reload_length & ADDRESS_MASK;
        self.sound_dma.cycle_accumulator = cycle_accumulator;
    }
}

fn sound_dma_cycle_period(control: u8) -> u32 {
    match control & 0x03 {
        0x00 => 768,
        0x01 => 512,
        0x02 => 256,
        _ => 128,
    }
}

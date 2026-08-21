use super::{HuC6270, VdcRegister, VdcStatus};

pub const VDC_SATB_WORDS: usize = 256;
pub const DETERMINISTIC_VDC_INITIAL_SATB_WORD: u16 = 0;
pub const DETERMINISTIC_VDC_RESET_CLEARS_SATB: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcDmaDirection {
    Increment,
    Decrement,
}

impl VdcDmaDirection {
    #[inline]
    const fn advance(self, address: u16) -> u16 {
        match self {
            Self::Increment => address.wrapping_add(1),
            Self::Decrement => address.wrapping_sub(1),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramDmaState {
    source: u16,
    destination: u16,
    remaining_words: u32,
    source_direction: VdcDmaDirection,
    destination_direction: VdcDmaDirection,
}

impl VramDmaState {
    #[inline]
    pub const fn source(self) -> u16 {
        self.source
    }

    #[inline]
    pub const fn destination(self) -> u16 {
        self.destination
    }

    #[inline]
    pub const fn remaining_words(self) -> u32 {
        self.remaining_words
    }

    #[inline]
    pub const fn source_direction(self) -> VdcDmaDirection {
        self.source_direction
    }

    #[inline]
    pub const fn destination_direction(self) -> VdcDmaDirection {
        self.destination_direction
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramSatbDmaState {
    source: u16,
    next_word: u16,
}

impl VramSatbDmaState {
    #[inline]
    pub const fn source(self) -> u16 {
        self.source
    }

    #[inline]
    pub const fn next_word(self) -> u16 {
        self.next_word
    }

    #[inline]
    pub const fn remaining_words(self) -> u16 {
        VDC_SATB_WORDS as u16 - self.next_word
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcDmaAccess {
    Source,
    Destination,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcDmaChannel {
    Vram,
    Satb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcDmaError {
    VramAddressUnavailable { address: u16, access: VdcDmaAccess },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcDmaProgress {
    Idle,
    Transferred { remaining_words: u32 },
    Complete,
}

impl HuC6270 {
    #[inline]
    pub const fn pending_vram_dma(&self) -> Option<VramDmaState> {
        self.vram_dma_pending
    }

    #[inline]
    pub const fn active_vram_dma(&self) -> Option<VramDmaState> {
        self.vram_dma_active
    }

    #[inline]
    pub const fn pending_satb_dma(&self) -> Option<VramSatbDmaState> {
        self.satb_dma_pending
    }

    #[inline]
    pub const fn active_satb_dma(&self) -> Option<VramSatbDmaState> {
        self.satb_dma_active
    }

    pub fn activate_pending_vram_dma(&mut self) -> bool {
        if self.vram_dma_active.is_some() {
            return false;
        }
        let Some(pending) = self.vram_dma_pending.take() else {
            return false;
        };
        self.vram_dma_active = Some(pending);
        true
    }

    #[inline]
    pub fn service_dma_slot(
        &mut self,
        channel: VdcDmaChannel,
    ) -> Result<VdcDmaProgress, VdcDmaError> {
        match channel {
            VdcDmaChannel::Vram => self.service_vram_dma_slot(),
            VdcDmaChannel::Satb => self.service_satb_dma_slot(),
        }
    }

    fn service_vram_dma_slot(&mut self) -> Result<VdcDmaProgress, VdcDmaError> {
        let Some(mut transfer) = self.vram_dma_active.take() else {
            return Ok(VdcDmaProgress::Idle);
        };

        let value = self.read_logical_vram_word(transfer.source);
        self.write_logical_vram_word(transfer.destination, value);

        transfer.source = transfer.source_direction.advance(transfer.source);
        transfer.destination = transfer.destination_direction.advance(transfer.destination);
        transfer.remaining_words -= 1;
        self.registers[VdcRegister::DmaSource as usize] = transfer.source;
        self.registers[VdcRegister::DmaDestination as usize] = transfer.destination;
        self.registers[VdcRegister::DmaLength as usize] =
            transfer.remaining_words.wrapping_sub(1) as u16;

        if transfer.remaining_words == 0 {
            if self.register(VdcRegister::DmaControl) & 0x02 != 0 {
                self.latch_status(VdcStatus::VRAM_DMA_COMPLETE);
            }
            Ok(VdcDmaProgress::Complete)
        } else {
            let remaining_words = transfer.remaining_words;
            self.vram_dma_active = Some(transfer);
            Ok(VdcDmaProgress::Transferred { remaining_words })
        }
    }

    #[inline]
    pub fn abort_vram_dma_for_active_display(&mut self) -> bool {
        let active = self.vram_dma_active.take().is_some();
        let pending = self.vram_dma_pending.take().is_some();
        active || pending
    }

    pub fn start_satb_dma_for_vertical_blank(&mut self) -> bool {
        if self.satb_dma_active.is_some() {
            return false;
        }

        let pending = self.satb_dma_pending.take();
        let transfer = pending.or_else(|| {
            (self.register(VdcRegister::DmaControl) & 0x10 != 0).then_some(VramSatbDmaState {
                source: self.register(VdcRegister::SatbSource),
                next_word: 0,
            })
        });
        let Some(transfer) = transfer else {
            return false;
        };
        self.satb_dma_active = Some(transfer);
        true
    }

    fn service_satb_dma_slot(&mut self) -> Result<VdcDmaProgress, VdcDmaError> {
        let Some(mut transfer) = self.satb_dma_active.take() else {
            return Ok(VdcDmaProgress::Idle);
        };
        let address = transfer.source.wrapping_add(transfer.next_word);
        let value = self.read_logical_vram_word(address);
        self.satb[usize::from(transfer.next_word)] = value;
        transfer.next_word += 1;

        if usize::from(transfer.next_word) == VDC_SATB_WORDS {
            if self.register(VdcRegister::DmaControl) & 0x01 != 0 {
                self.latch_status(VdcStatus::SATB_DMA_COMPLETE);
            }
            Ok(VdcDmaProgress::Complete)
        } else {
            let remaining_words = u32::from(transfer.remaining_words());
            self.satb_dma_active = Some(transfer);
            Ok(VdcDmaProgress::Transferred { remaining_words })
        }
    }

    pub(crate) fn queue_vram_dma(&mut self) {
        let control = self.register(VdcRegister::DmaControl);
        self.vram_dma_pending = Some(VramDmaState {
            source: self.register(VdcRegister::DmaSource),
            destination: self.register(VdcRegister::DmaDestination),
            remaining_words: u32::from(self.register(VdcRegister::DmaLength)) + 1,
            source_direction: if control & 0x04 == 0 {
                VdcDmaDirection::Increment
            } else {
                VdcDmaDirection::Decrement
            },
            destination_direction: if control & 0x08 == 0 {
                VdcDmaDirection::Increment
            } else {
                VdcDmaDirection::Decrement
            },
        });
    }

    pub(super) fn queue_satb_dma(&mut self) {
        self.satb_dma_pending = Some(VramSatbDmaState {
            source: self.register(VdcRegister::SatbSource),
            next_word: 0,
        });
    }
}

use super::*;
use crate::cartridge::SampledByte;

pub(super) const MAX_EFFECTS: usize = 12;

pub(crate) trait Storage {
    fn vram(&self, address: u16) -> Result<u8>;
    fn cram(&self, index: u8) -> Result<u16>;
    fn vsram(&self, index: u8) -> Result<u16>;
    fn main_byte(&self, address: u32) -> Result<u8>;
    fn main_byte_sampled(&self, address: u32) -> Result<SampledByte> {
        self.main_byte(address).map(SampledByte::unknown)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    Vram(ram::ByteWrite),
    Internal(internal_ram::Write),
    WorkRam { address: u16, value: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub half: u64,
    pub effects: Vec<Effect>,
    pub dma_activity: bool,
    pub pixel: Option<internal_ram::VideoPixel>,
}

pub(super) struct Overlay<'a, S> {
    pub storage: &'a S,
    pub effects: &'a [Effect],
}

impl<S: Storage> Storage for Overlay<'_, S> {
    fn main_byte(&self, address: u32) -> Result<u8> {
        self.main_byte_sampled(address).map(|sample| sample.value)
    }

    fn main_byte_sampled(&self, address: u32) -> Result<SampledByte> {
        if (0xe00000..=0xffffff).contains(&address) {
            for effect in self.effects.iter().rev() {
                if let Effect::WorkRam {
                    address: stored,
                    value,
                } = effect
                    && *stored == address as u16
                {
                    return Ok(SampledByte::unknown(*value));
                }
            }
        }
        self.storage.main_byte_sampled(address)
    }
    fn vram(&self, address: u16) -> Result<u8> {
        for effect in self.effects.iter().rev() {
            if let Effect::Vram(write) = effect
                && write.canonical_address == address
            {
                return Ok(write.value);
            }
        }
        self.storage.vram(address)
    }

    fn cram(&self, index: u8) -> Result<u16> {
        ensure!(index < 64, "invalid CRAM storage index");
        self.word(internal_ram::Target::Cram, index, self.storage.cram(index)?)
    }

    fn vsram(&self, index: u8) -> Result<u16> {
        ensure!(index < 40, "invalid VSRAM storage index");
        self.word(
            internal_ram::Target::Vsram,
            index,
            self.storage.vsram(index)?,
        )
    }
}

impl<S> Overlay<'_, S> {
    fn word(&self, target: internal_ram::Target, index: u8, mut value: u16) -> Result<u16> {
        let limit = match target {
            internal_ram::Target::Cram => 0xeee,
            internal_ram::Target::Vsram => 0x7ff,
        };
        ensure!(value & !limit == 0, "invalid parent internal VDP RAM word");
        for effect in self.effects {
            if let Effect::Internal(write) = effect
                && write.target == target
                && write.index == index
            {
                value = (value & !write.mask) | write.value;
            }
        }
        Ok(value)
    }
}

use super::{Cartridge, sf002};
use zeff_m68k::timing::InstructionByteOrigin;

#[derive(Clone, Copy, Debug)]
pub(crate) struct SampledByte {
    pub value: u8,
    pub origin: InstructionByteOrigin,
}

impl SampledByte {
    pub(crate) fn unknown(value: u8) -> Self {
        Self {
            value,
            origin: InstructionByteOrigin::UNKNOWN,
        }
    }

    fn storage(value: u8, offset: usize) -> Self {
        Self {
            value,
            origin: InstructionByteOrigin::storage(offset as u64)
                .expect("cartridge source offset fits origin"),
        }
    }
}

impl From<u8> for SampledByte {
    fn from(value: u8) -> Self {
        Self::unknown(value)
    }
}

impl Cartridge {
    pub fn read(&self, address: u32) -> Option<u8> {
        self.read_sampled(address).map(|byte| byte.value)
    }

    pub(crate) fn source_sample(&self, offset: usize) -> Option<SampledByte> {
        self.source_byte(offset)
            .map(|value| SampledByte::storage(value, offset))
    }

    pub(crate) fn read_sampled(&self, address: u32) -> Option<SampledByte> {
        if let Some(svp) = &self.svp {
            return match address {
                0..=0x1fffff => self.source_sample(address as usize),
                0x300000..=0x31ffff => {
                    let word = svp.peek_dram(((address - 0x300000) >> 1) as u16);
                    Some(SampledByte::unknown(
                        word.to_be_bytes()[address as usize & 1],
                    ))
                }
                _ => None,
            };
        }
        if let Some(board) = &self.download {
            return board
                .read(address)
                .map(|value| self.rom_overlay.read(address as usize, value))
                .map(SampledByte::unknown);
        }
        if self.save_latch.is_some() && address < 0x400000 {
            return self.save_latch_rom_offset(address).map_or_else(
                || self.read_save_latch(address).map(SampledByte::unknown),
                |offset| self.source_sample(offset),
            );
        }
        if self.bounded_switched_guarded(address) {
            let save = self.save.as_ref()?;
            return if save.rom_visible() {
                self.source_sample(address as usize)
            } else {
                save.read(address).map(SampledByte::unknown)
            };
        }
        if let Some(offset) = self
            .registers
            .as_ref()
            .and_then(|board| board.source_offset(address))
        {
            return self.source_sample(offset);
        }
        if let Some(board) = &self.realtec {
            return board
                .source_offset(address)
                .and_then(|offset| self.source_sample(offset));
        }
        if let Some(board) = &self.extended_ssf {
            return board
                .source_offset(address)
                .and_then(|at| self.source_sample(at));
        }
        if self.parallel_adapter_domain(address) || self.bounded_split_hole(address) {
            return None;
        }
        if let Some(board) = &self.sf004 {
            return board.source_offset(address).map_or_else(
                || board.read(address, &self.bytes).map(SampledByte::unknown),
                |offset| self.source_sample(offset),
            );
        }
        if self.linear_rom.is_some() {
            return self.source_sample(address as usize);
        }
        if let Some(board) = &self.sf002 {
            return match board.decode(address) {
                sf002::Location::Rom(offset) => self.source_sample(offset),
                sf002::Location::Unclaimed => {
                    self.save.as_ref()?.read(address).map(SampledByte::unknown)
                }
                sf002::Location::Unsupported(_) => None,
            };
        }
        if let Some(board) = &self.ssf2 {
            return board
                .source_offset(address)
                .and_then(|offset| self.source_sample(offset));
        }
        if let Some(save) = &self.lock_on_save
            && save.enabled
            && save.contains(address)
        {
            return save.read(address).map(SampledByte::unknown);
        }
        if let Some(board) = &self.sf001 {
            let mut sampled = None;
            let value = board.read(address, |offset| {
                sampled = self.source_sample(offset);
                sampled.map(|byte| byte.value)
            })?;
            return Some(sampled.unwrap_or_else(|| SampledByte::unknown(value)));
        }
        if let Some(save) = &self.save
            && save.contains(address)
            && !save.rom_visible()
        {
            return save.read(address).map(SampledByte::unknown);
        }
        if (0x200000..=0x3fffff).contains(&address)
            && let Some(mapping) = self.lock_on
        {
            let mut sampled = None;
            let value = mapping.read(address, |offset| {
                let byte = self
                    .source_sample(offset)
                    .expect("qualified lock-on source");
                sampled = Some(byte);
                byte.value
            });
            return Some(sampled.unwrap_or_else(|| SampledByte::unknown(value)));
        }
        Some(
            self.source_sample(address as usize)
                .unwrap_or_else(|| SampledByte::unknown(crate::constants::UNPOPULATED_ROM_BYTE)),
        )
    }
}

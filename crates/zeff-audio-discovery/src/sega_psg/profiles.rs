use zeff_emu_common::system::System;

use super::{RomSpan, SegaPsgRegion};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HeaderLayout {
    FourByteChannels,
    SixByteChannels,
}

pub(super) struct RejectedSelector {
    pub raw_index: u8,
    pub reason: &'static str,
}

pub(super) struct SupplementalSelector {
    pub raw_index: u8,
    pub spans: &'static [RomSpan],
}

pub(super) struct Profile {
    pub name: &'static str,
    pub sha256: &'static str,
    pub rom_len: usize,
    pub system: System,
    pub region: SegaPsgRegion,
    pub audio_offset: usize,
    pub driver_address: u16,
    pub init_address: u16,
    pub table: u16,
    // The adjacent SFX table can begin before the dispatcher's reserved music range ends.
    pub song_count: u8,
    pub frame_divider: u8,
    pub audio_byte_len: usize,
    pub header_layout: HeaderLayout,
    pub rejected_selectors: &'static [RejectedSelector],
    pub supplemental_selectors: &'static [SupplementalSelector],
}

impl Profile {
    pub fn audio_len(&self) -> usize {
        self.audio_byte_len
    }

    pub fn rejected_selector(&self, raw_index: u8) -> Option<&RejectedSelector> {
        self.rejected_selectors
            .iter()
            .find(|rejected| rejected.raw_index == raw_index)
    }

    pub fn supplemental_spans(&self, raw_index: u8) -> &[RomSpan] {
        self.supplemental_selectors
            .iter()
            .find(|selector| selector.raw_index == raw_index)
            .map_or(&[], |selector| selector.spans)
    }
}

mod four_byte;
mod six_byte;

pub(super) fn all_profiles() -> impl Iterator<Item = &'static Profile> {
    four_byte::PROFILES.iter().chain(six_byte::PROFILES.iter())
}

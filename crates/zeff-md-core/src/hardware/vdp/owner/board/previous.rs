use super::*;
use crate::constants::WORK_RAM_START;
use zeff_m68k::bus::BUS_ADDRESS_MASK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    pub request: main_bus::Request,
    pub start_half: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct Tag {
    pub key: Key,
    pub half: u64,
    pub ordinal: usize,
    pub lane: usize,
    pub address: u16,
    pub value: u8,
}

pub(crate) trait TagSink: Clone + Default {
    const ENABLED: bool;
    fn record(&mut self, tag: Tag);
}

impl TagSink for () {
    const ENABLED: bool = false;
    fn record(&mut self, _: Tag) {}
}

#[derive(Clone, Default)]
pub(crate) struct Tags(pub [Option<Tag>; 2]);

impl TagSink for Tags {
    const ENABLED: bool = true;
    fn record(&mut self, tag: Tag) {
        if let Some(slot) = self.0.get_mut(tag.ordinal) {
            *slot = Some(tag);
        }
    }
}

#[derive(Default)]
pub(super) struct Sources {
    pub address: bool,
    pub data: [bool; 2],
}

impl Sources {
    pub(super) fn tag(
        &self,
        board: &Board,
        stage: Stage,
        released: bool,
        position: (u64, usize),
        effect: (usize, u16, u8),
    ) -> Option<Tag> {
        let (half, ordinal) = position;
        let (lane, address, value) = effect;
        let pending = board.snapshot()?.pending?;
        let main_bus::Operation::Write(word) = pending.request.operation else {
            return None;
        };
        let access = pending.request.access;
        let selected = match lane {
            0 => access.lanes != ByteLanes::Lower && stage.pins.upper_strobe,
            1 => access.lanes != ByteLanes::Upper && stage.pins.lower_strobe,
            _ => false,
        };
        // A lane belongs to MAIN only while its final address/data selection survives.
        if !self.address
            || self.data.get(lane).copied() != Some(true)
            || !selected
            || released
            || board.protocol.is_some()
            || board.protocol_drive.is_some()
            || pending.vpa.is_some()
            || pending.start_half > half
            || stage.pins.read
            || !stage.pins.address_strobe
            || stage.pins.interrupt_acknowledge
            || stage.pins.address != Some(access.bus_address)
            || stage.pins.write_word != Some(word)
            || stage.pins.write_lanes != Some(access.lanes)
            || stage.pins.function_code != access.function_code
            || board.address != access.bus_address
            || !(WORK_RAM_START..=BUS_ADDRESS_MASK).contains(&access.bus_address)
            || address != (access.bus_address as u16).wrapping_add(lane as u16)
            || value != word.to_be_bytes()[lane]
        {
            return None;
        }
        Some(Tag {
            key: Key {
                request: pending.request,
                start_half: pending.start_half,
            },
            half,
            ordinal,
            lane,
            address,
            value,
        })
    }
}

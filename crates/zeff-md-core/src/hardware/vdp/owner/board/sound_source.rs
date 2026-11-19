use super::*;
use crate::sound_trace::physical::{Stamp, WorkTag};

#[derive(Default)]
pub(super) struct Sources {
    pub address: u32,
    pub data: [bool; 2],
}
impl Sources {
    pub fn clear_data_mask(&mut self, mask: u16) {
        if mask & 0xff00 != 0 {
            self.data[0] = false;
        }
        if mask & 0x00ff != 0 {
            self.data[1] = false;
        }
    }
    pub fn tag(
        &self,
        stage: Stage,
        position: (Stamp, usize),
        effect: (usize, u16, u8),
    ) -> Option<WorkTag> {
        let (stamp, ordinal) = position;
        let (lane, address, value) = effect;
        let pins = stage.companion.pins;
        if self.address != 0xfffffe
            || self.data.get(lane).copied() != Some(true)
            || !pins.shared()
            || !pins.write
            || pins.data.is_none()
        {
            return None;
        }
        Some(WorkTag {
            stamp,
            ordinal,
            lane,
            logical: pins.address?,
            bank: stage.companion.bank,
            address,
            value,
        })
    }
}

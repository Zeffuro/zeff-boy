use super::model::Failure;
use super::{BusAccess, Direction, InstructionPhase};

pub(super) const SLOTS: usize = 48;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ram([Option<(u32, u16)>; SLOTS]);
impl Default for Ram {
    fn default() -> Self {
        Self([None; SLOTS])
    }
}
impl Ram {
    pub fn insert(&mut self, key: u32, value: u16) {
        let key = key & zeff_m68k::bus::BUS_ADDRESS_MASK & !1;
        let at = self
            .0
            .iter()
            .position(|entry| entry.is_some_and(|(address, _)| address == key))
            .or_else(|| self.0.iter().position(Option::is_none))
            .expect("RAM capacity");
        self.0[at] = Some((key, value));
    }
    pub fn sample(&self, key: u32) -> u16 {
        self.0
            .iter()
            .flatten()
            .find(|(address, _)| *address == key & zeff_m68k::bus::BUS_ADDRESS_MASK & !1)
            .map_or(0, |(_, value)| *value)
    }
    pub fn iter(&self) -> impl Iterator<Item = &(u32, u16)> {
        self.0.iter().flatten()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Transfer {
        stage: u8,
        phase: Option<InstructionPhase>,
        access: BusAccess,
        direction: Direction,
        value: Option<u16>,
        start: u64,
        elapsed: u32,
        kind: Failure,
        deposited: bool,
    },
    Idle(u64, u32),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Events {
    slots: [Option<Event>; SLOTS],
    pub len: usize,
}
impl Default for Events {
    fn default() -> Self {
        Self {
            slots: [None; SLOTS],
            len: 0,
        }
    }
}
impl Events {
    pub fn push(&mut self, event: Event) {
        assert!(self.len < SLOTS, "event capacity");
        self.slots[self.len] = Some(event);
        self.len += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.slots[..self.len].iter().flatten()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scripts {
    slots: [Option<(u8, Failure, u32)>; SLOTS],
    pub len: usize,
}
impl Default for Scripts {
    fn default() -> Self {
        Self {
            slots: [None; SLOTS],
            len: 0,
        }
    }
}
impl Scripts {
    pub fn iter(&self) -> impl Iterator<Item = &(u8, Failure, u32)> {
        self.slots[..self.len].iter().flatten()
    }
    pub fn push(&mut self, stage: u8, kind: Failure, elapsed: u32) {
        assert!(self.len < SLOTS, "script capacity");
        self.slots[self.len] = Some((stage, kind, elapsed));
        self.len += 1;
    }
}

use super::*;
use device::{Event, Kind, Stage};
pub(super) const RAM_SLOTS: usize = 32;
pub(super) const EVENT_SLOTS: usize = 32;
pub(super) const SCRIPT_SLOTS: usize = 32;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ram {
    slots: [Option<(u32, u16)>; RAM_SLOTS],
}
impl Default for Ram {
    fn default() -> Self {
        Self {
            slots: [None; RAM_SLOTS],
        }
    }
}
impl Ram {
    pub fn insert(&mut self, key: u32, value: u16) {
        let key = key & BUS_ADDRESS_MASK & !1;
        let at = self
            .slots
            .iter()
            .position(|s| s.is_some_and(|(a, _)| a == key))
            .or_else(|| self.slots.iter().position(Option::is_none))
            .expect("RAM capacity");
        self.slots[at] = Some((key, value));
    }
    pub fn get(&self, key: &u32) -> Option<&u16> {
        self.slots
            .iter()
            .flatten()
            .find(|(a, _)| a == key)
            .map(|(_, v)| v)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Events {
    slots: [Option<Event>; EVENT_SLOTS],
    pub len: usize,
}
impl Default for Events {
    fn default() -> Self {
        Self {
            slots: [None; EVENT_SLOTS],
            len: 0,
        }
    }
}
impl Events {
    pub fn push(&mut self, event: Event) {
        assert!(self.len < EVENT_SLOTS);
        self.slots[self.len] = Some(event);
        self.len += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.slots[..self.len].iter().flatten()
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scripts {
    slots: [Option<(Stage, Kind, u32)>; SCRIPT_SLOTS],
    pub len: usize,
}
impl Default for Scripts {
    fn default() -> Self {
        Self {
            slots: [None; SCRIPT_SLOTS],
            len: 0,
        }
    }
}
impl Scripts {
    pub fn push(&mut self, script: (Stage, Kind, u32)) {
        assert!(self.len < SCRIPT_SLOTS);
        self.slots[self.len] = Some(script);
        self.len += 1;
    }
}

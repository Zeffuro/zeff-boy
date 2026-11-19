use super::*;
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
        let key = key & BUS_ADDRESS_MASK & !1;
        let at = self
            .0
            .iter()
            .position(|s| s.is_some_and(|(a, _)| a == key))
            .or_else(|| self.0.iter().position(Option::is_none))
            .expect("RAM capacity");
        self.0[at] = Some((key, value));
    }
    pub fn sample(&self, key: u32) -> u16 {
        self.0
            .iter()
            .flatten()
            .find(|(a, _)| *a == key & BUS_ADDRESS_MASK & !1)
            .map_or(0x4e71, |(_, v)| *v)
    }
    pub fn memory(&self) -> Memory {
        let mut memory = Memory::new();
        for &(key, value) in self.0.iter().flatten() {
            memory.insert(key, value);
        }
        memory
    }
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
    pub fn push(&mut self, e: Event) {
        assert!(self.len < SLOTS, "event capacity");
        self.slots[self.len] = Some(e);
        self.len += 1;
    }
}

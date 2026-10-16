#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Memory {
    entries: [(u32, u16); 64],
    len: usize,
}
impl Memory {
    pub fn new() -> Self {
        Self {
            entries: [(0, 0); 64],
            len: 0,
        }
    }
    pub fn get(&self, address: &u32) -> Option<&u16> {
        self.entries[..self.len]
            .iter()
            .find(|(a, _)| a == address)
            .map(|(_, v)| v)
    }
    pub fn insert(&mut self, address: u32, value: u16) {
        if let Some((_, old)) = self.entries[..self.len]
            .iter_mut()
            .find(|(a, _)| *a == address)
        {
            *old = value;
        } else {
            assert!(self.len < 64);
            self.entries[self.len] = (address, value);
            self.len += 1;
        }
    }
}

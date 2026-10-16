use zeff_m68k::bus::{BUS_ADDRESS_MASK, BusAccess, ByteLanes};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixed<T: Copy> {
    pub slots: [Option<T>; 48],
    pub len: usize,
}

impl<T: Copy> Default for Fixed<T> {
    fn default() -> Self {
        Self {
            slots: [None; 48],
            len: 0,
        }
    }
}

impl<T: Copy> Fixed<T> {
    pub fn push(&mut self, value: T) {
        self.slots[self.len] = Some(value);
        self.len += 1;
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.slots[..self.len].iter().flatten()
    }
}

impl Fixed<(u32, u16)> {
    pub fn sample(&self, address: u32) -> u16 {
        let key = address & BUS_ADDRESS_MASK & !1;
        self.iter()
            .find(|(old, _)| *old == key)
            .map_or(0, |(_, value)| *value)
    }

    pub fn insert(&mut self, address: u32, value: u16) {
        let key = address & BUS_ADDRESS_MASK & !1;
        if let Some(slot) = self.slots[..self.len]
            .iter_mut()
            .find(|slot| slot.is_some_and(|(old, _)| old == key))
        {
            *slot = Some((key, value));
        } else {
            self.push((key, value));
        }
    }

    pub fn deposit(&mut self, access: BusAccess, value: u16) {
        let old = self.sample(access.bus_address);
        self.insert(
            access.bus_address,
            match access.lanes {
                ByteLanes::Upper => old & 0x00ff | value & 0xff00,
                ByteLanes::Lower => old & 0xff00 | value & 0x00ff,
                ByteLanes::Both => value,
            },
        );
    }
}

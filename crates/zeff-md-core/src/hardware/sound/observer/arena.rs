use super::ledger::CAPACITY;

pub(super) struct Arena<T: Copy> {
    values: [Option<T>; CAPACITY],
    ids: [u64; CAPACITY],
    free: [usize; CAPACITY],
    available: usize,
}

impl<T: Copy> Arena<T> {
    pub const fn new() -> Self {
        let mut free = [0; CAPACITY];
        let mut index = 0;
        while index < CAPACITY {
            free[index] = CAPACITY - index - 1;
            index += 1;
        }
        Self {
            values: [None; CAPACITY],
            ids: [0; CAPACITY],
            free,
            available: CAPACITY,
        }
    }

    pub fn insert(&mut self, value: T) -> Option<u64> {
        let free = self.available.checked_sub(1)?;
        let index = self.free[free];
        let id = if self.ids[index] == 0 {
            index as u64 + 1
        } else {
            self.ids[index].checked_add(CAPACITY as u64)?
        };
        self.available = free;
        self.ids[index] = id;
        self.values[index] = Some(value);
        Some(id)
    }

    fn index(&self, id: u64) -> Option<usize> {
        let index = (id.checked_sub(1)? % CAPACITY as u64) as usize;
        (self.ids[index] == id && self.values[index].is_some()).then_some(index)
    }

    pub fn get(&self, id: u64) -> Option<&T> {
        self.values[self.index(id)?].as_ref()
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut T> {
        let index = self.index(id)?;
        self.values[index].as_mut()
    }

    pub fn remove(&mut self, id: u64) -> Option<T> {
        let index = self.index(id)?;
        let value = self.values[index].take();
        self.free[self.available] = index;
        self.available += 1;
        value
    }

    #[cfg(any(test, feature = "ram-copy-census"))]
    pub fn len(&self) -> usize {
        CAPACITY - self.available
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn observer_arena_reuse_rejects_stale_handles_with_live_neighbors() {
        let mut arena = Arena::new();
        let handles: Vec<_> = (0..CAPACITY).map(|n| arena.insert(n).unwrap()).collect();
        assert_eq!(arena.insert(0), None);
        let stale = handles[CAPACITY / 2];
        assert_eq!(arena.remove(stale), Some(CAPACITY / 2));
        for value in 0..3 * CAPACITY {
            let current = arena.insert(value).unwrap();
            assert_ne!(current, stale);
            assert_eq!(arena.get(stale), None);
            assert_eq!(arena.remove(stale), None);
            assert_eq!(arena.get(current), Some(&value));
            assert_eq!(arena.len(), CAPACITY);
            assert_eq!(arena.remove(current), Some(value));
            assert_eq!(arena.remove(current), None);
        }
        for (n, &id) in handles.iter().enumerate() {
            if id != stale {
                assert_eq!(arena.remove(id), Some(n));
            }
        }
        assert_eq!(arena.len(), 0);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn observer_arena_uses_full_width_identity_before_index_conversion() {
        let mut arena = Arena::new();
        arena.ids[0] = u64::from(u32::MAX) + 2;
        let id = arena.insert(7).unwrap();
        assert!(id > u64::from(u32::MAX));
        assert_eq!(arena.get(id), Some(&7));
        assert_eq!(arena.get(1), None);
        assert_eq!(arena.remove(id), Some(7));
        assert_eq!(arena.len(), 0);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn observer_arena_identity_overflow_never_reissues_old_handle() {
        let mut arena = Arena::new();
        arena.ids[0] = u64::MAX - (CAPACITY as u64 - 2);
        let old = arena.ids[0];
        assert_eq!(arena.insert(7), None);
        assert_eq!(arena.insert(8), None);
        assert_eq!(arena.get(old), None);
        assert_eq!(arena.get(1), None);
        assert_eq!(arena.len(), 0);
    }
}

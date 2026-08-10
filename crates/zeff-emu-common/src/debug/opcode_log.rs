const OPCODE_LOG_CAPACITY: usize = 32;
const OPCODE_LOG_MASK: usize = OPCODE_LOG_CAPACITY - 1;

pub struct OpcodeLog<E: Copy + Default> {
    entries: [E; OPCODE_LOG_CAPACITY],
    cursor: usize,
    count: usize,
    pub enabled: bool,
}

impl<E: Copy + Default> std::fmt::Debug for OpcodeLog<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpcodeLog")
            .field("count", &self.count)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl<E: Copy + Default> Default for OpcodeLog<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Copy + Default> OpcodeLog<E> {
    pub fn new() -> Self {
        Self {
            entries: core::array::from_fn(|_| E::default()),
            cursor: 0,
            count: 0,
            enabled: false,
        }
    }

    #[inline]
    pub fn push(&mut self, entry: E) {
        if !self.enabled {
            return;
        }
        self.entries[self.cursor] = entry;
        self.cursor = (self.cursor + 1) & OPCODE_LOG_MASK;
        if self.count < OPCODE_LOG_CAPACITY {
            self.count += 1;
        }
    }

    pub fn recent(&self, n: usize) -> Vec<E> {
        let take = n.min(self.count);
        let mut result = Vec::with_capacity(take);
        for i in 0..take {
            let idx = (self.cursor.wrapping_sub(1 + i)) & OPCODE_LOG_MASK;
            result.push(self.entries[idx]);
        }
        result
    }

    pub fn clear(&mut self) {
        self.count = 0;
        self.cursor = 0;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

use super::*;

impl CycleRequest {
    pub fn sampled_t_states(&self, sample: CycleSample) -> Result<u32, CycleError> {
        if sample.address != self.kind.address()
            || sample.m1 != self.kind.is_m1()
            || sample.write != self.kind.is_write()
        {
            return Err(CycleError::SampleMismatch);
        }
        if sample.known_mask != u8::MAX {
            return Err(CycleError::UnknownData);
        }
        if let CycleKind::MemoryWrite { value, .. } | CycleKind::MemoryWriteExtended { value, .. } =
            self.kind
            && sample.value != value
        {
            return Err(CycleError::SampleMismatch);
        }
        match self.kind.t_states() {
            Some(cycles) => Ok(cycles),
            None => self
                .cpu
                .sampled_m1_t_states(&self.bus, self.kind, sample.value),
        }
    }
}

impl CycleCompletion {
    pub fn t_states(&self) -> u32 {
        self.base_t_states
    }
    pub fn wait_states(&self) -> u32 {
        self.tw
    }
}

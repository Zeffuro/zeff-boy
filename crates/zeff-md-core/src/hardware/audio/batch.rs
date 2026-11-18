use super::*;

const MAX_DEFERRED_TICKS: u64 =
    (SAMPLE_CAPACITY as u64 / 2) * crate::constants::PAL_MASTER_HZ / MAX_SAMPLE_RATE as u64;

impl Audio {
    pub(crate) fn batch_active(&self) -> bool {
        self.deferred.is_some()
    }

    pub(crate) fn begin_batch(&mut self) {
        assert!(self.deferred.is_none());
        self.deferred = Some(self.now);
    }

    pub(crate) fn finish_batch(&mut self) {
        let at = self.deferred.take().expect("active audio batch");
        if at > self.now {
            self.advance_to(at);
        }
    }

    pub(crate) fn flush_pending(&mut self) {
        if let Some(at) = self.deferred
            && at > self.now
        {
            self.advance_to(at);
        }
    }

    pub(crate) fn request_advance_to(&mut self, at: u64) {
        if self.generation_enabled
            && at.saturating_sub(self.now) <= MAX_DEFERRED_TICKS
            && let Some(admitted) = &mut self.deferred
        {
            assert!(at >= *admitted, "ordered audio admission");
            *admitted = at;
            return;
        }
        self.flush_pending();
        self.advance_to(at);
    }
}

#[cfg(test)]
mod tests;

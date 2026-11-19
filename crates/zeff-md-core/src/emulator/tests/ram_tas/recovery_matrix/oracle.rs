use super::super::sound_oracle::{CasePolicy, Oracle as SharedOracle};
use super::*;
use crate::hardware::bus::Bus;

pub(super) type Oracle = SharedOracle<Case>;

impl CasePolicy for Case {
    const GATE_RESET_REQUEST: bool = true;

    fn prefix_t(&self) -> u64 {
        Case::prefix_t(self)
    }

    fn expected_target(&self) -> (u32, bool, bool) {
        Case::expected_target(self)
    }

    fn after_publication(&self, _: &mut Bus, _: bool) {}
}

impl SharedOracle<Case> {
    pub fn clear_preparation_checkpoint(&mut self) {
        self.checkpoint = None;
        self.deferrals = 0;
    }

    pub fn vdp_advance(&mut self, end: u64, release: u64) {
        let checkpoint = self.checkpoint.clone();
        let deferrals = self.deferrals;
        self.advance(end, Some((self.bus.now.get(), release, true)));
        self.checkpoint = checkpoint;
        self.deferrals = deferrals;
    }

    pub fn end(&mut self) {
        let (a, e) = self.lock.take().unwrap();
        let now = self.bus.now.get();
        if let Some(checkpoint) = self.checkpoint.take()
            && now < e
            && self.pending.is_some()
        {
            self.pending = Some(checkpoint);
            let deferrals = self.deferrals;
            self.reserve(Some((a, now.clamp(a, e), true)));
            self.deferrals = deferrals;
            self.checkpoint = None;
            if let Some(p) = &self.pending
                && p.target
            {
                self.target = Some((p.start, p.end));
            }
        }
    }
}

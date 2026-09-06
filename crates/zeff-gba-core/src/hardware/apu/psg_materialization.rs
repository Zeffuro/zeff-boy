use std::borrow::Cow;

use super::{Apu, Psg};

impl Apu {
    pub(super) fn refresh_psg_merge_horizon(&mut self) {
        self.psg_merge_horizon_t_cycles = self.psg.merge_horizon_t_cycles();
    }

    fn run_psg(&mut self, t_cycles: u64) {
        debug_assert_ne!(t_cycles, 0);
        self.psg.step(t_cycles);
    }

    #[cfg(test)]
    pub(super) fn step_psg_eager(&mut self, cycles: u32) {
        debug_assert_eq!(self.psg_pending_t_cycles, 0);
        let t_cycles = self.consume_psg_cycles(cycles);
        if t_cycles != 0 {
            self.run_psg(t_cycles);
        }
    }

    pub(super) fn step_psg_deferred(&mut self, cycles: u32, no_dac_samples: bool) {
        let t_cycles = self.consume_psg_cycles(cycles);
        if self.psg_merge_horizon_t_cycles == 0 {
            self.refresh_psg_merge_horizon();
        }
        let pending = self.psg_pending_t_cycles + t_cycles;
        if no_dac_samples
            && !self.debug_capture_enabled
            && pending < self.psg_merge_horizon_t_cycles
        {
            self.psg_pending_t_cycles = pending;
            return;
        }

        self.flush_pending_psg();
        if t_cycles != 0 {
            self.run_psg(t_cycles);
            self.refresh_psg_merge_horizon();
        }
    }

    pub(super) fn flush_pending_psg(&mut self) {
        let pending = self.psg_pending_t_cycles;
        if pending == 0 {
            return;
        }
        debug_assert!(pending < self.psg_merge_horizon_t_cycles);
        self.run_psg(pending);
        self.psg_pending_t_cycles = 0;
        self.refresh_psg_merge_horizon();
    }

    pub(super) fn projected_psg(&self) -> Cow<'_, Psg> {
        if self.psg_pending_t_cycles == 0 {
            return Cow::Borrowed(&self.psg);
        }
        debug_assert!(self.psg_pending_t_cycles < self.psg_merge_horizon_t_cycles);
        let mut projected = self.psg.clone();
        projected.step(self.psg_pending_t_cycles);
        Cow::Owned(projected)
    }

    pub(super) fn observed_psg(&self) -> &Psg {
        // Only registers and histories are invariant here; phase/output reads must materialize.
        debug_assert!(
            self.psg_pending_t_cycles == 0
                || self.psg_pending_t_cycles < self.psg_merge_horizon_t_cycles
        );
        &self.psg
    }

    #[cfg(test)]
    pub(crate) fn set_deferred_psg_for_test(&mut self, enabled: bool) {
        self.flush_pending_psg();
        self.deferred_psg_enabled = enabled;
        self.refresh_psg_merge_horizon();
    }

    #[cfg(test)]
    pub(crate) fn deferred_psg_pending_t_cycles_for_test(&self) -> u64 {
        self.psg_pending_t_cycles
    }
}

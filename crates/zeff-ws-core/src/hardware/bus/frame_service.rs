use super::Bus;
use crate::hardware::constants::CYCLES_PER_SCANLINE;

impl Bus {
    pub(crate) fn begin_frame_service(&mut self) {
        debug_assert!(!self.frame_service_deferred);
        debug_assert_eq!(self.frame_service_pending_cycles, 0);
        self.frame_service_deferred = true;
        self.frame_service_horizon = 0;
        #[cfg(feature = "profiling")]
        {
            self.profiling.frame_service_frames =
                self.profiling.frame_service_frames.wrapping_add(1);
        }
    }

    pub(crate) fn end_frame_service(&mut self) {
        self.materialize_frame_service();
        self.frame_service_deferred = false;
    }

    #[inline]
    pub(super) fn defer_frame_service(&mut self, cycles: u32) -> bool {
        if self.frame_service_horizon == 0 {
            self.frame_service_horizon = self.fresh_frame_service_horizon();
        }
        let pending = u64::from(self.frame_service_pending_cycles) + u64::from(cycles);
        if pending >= u64::from(self.frame_service_horizon) {
            #[cfg(feature = "profiling")]
            {
                self.profiling.frame_service_horizon_crossings = self
                    .profiling
                    .frame_service_horizon_crossings
                    .wrapping_add(1);
            }
            return false;
        }

        self.frame_service_pending_cycles = pending as u32;
        #[cfg(feature = "profiling")]
        {
            self.profiling.frame_service_deferred_calls =
                self.profiling.frame_service_deferred_calls.wrapping_add(1);
            self.profiling.frame_service_deferred_cycles = self
                .profiling
                .frame_service_deferred_cycles
                .wrapping_add(u64::from(cycles));
            self.profiling.frame_service_max_pending_cycles = self
                .profiling
                .frame_service_max_pending_cycles
                .max(self.frame_service_pending_cycles);
        }
        true
    }

    fn fresh_frame_service_horizon(&self) -> u32 {
        let mut horizon = CYCLES_PER_SCANLINE - self.ppu.line_cycles();
        if self.apu.sample_generation_enabled() {
            horizon = horizon.min(self.apu.cycles_until_next_sample());
        }
        let serial_control = self.io[usize::from(super::SERIAL_CONTROL_PORT)];
        if let Some(cycles) = self.uart.cycles_until_tx_complete(serial_control) {
            horizon = horizon.min(cycles);
        }
        if let Some(cycles) = self.cycles_until_next_sound_dma_transfer() {
            horizon = horizon.min(cycles);
        }
        horizon.max(1)
    }

    pub(crate) fn materialize_frame_service(&mut self) {
        self.frame_service_horizon = 0;
        let pending = std::mem::take(&mut self.frame_service_pending_cycles);
        if pending == 0 {
            return;
        }
        #[cfg(feature = "profiling")]
        {
            self.profiling.frame_service_materializations = self
                .profiling
                .frame_service_materializations
                .wrapping_add(1);
        }
        self.service_cycles(pending);
    }

    pub(super) fn fence_frame_service_io(&mut self) {
        #[cfg(feature = "profiling")]
        if self.frame_service_deferred {
            self.profiling.frame_service_io_fences =
                self.profiling.frame_service_io_fences.wrapping_add(1);
        }
        self.materialize_frame_service();
    }

    #[cfg(test)]
    pub(crate) fn frame_service_state_for_test(&self) -> (bool, u32, u32) {
        (
            self.frame_service_deferred,
            self.frame_service_pending_cycles,
            self.frame_service_horizon,
        )
    }
}

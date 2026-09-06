use super::Bus;
use crate::hardware::cartridge::BackupKind;

impl Bus {
    pub(crate) fn begin_frame_service(&mut self) {
        debug_assert!(!self.frame_service_deferred);
        debug_assert_eq!(self.frame_service_pending_cycles, 0);
        // RTC/EEPROM reads observe their clocks directly.
        self.frame_service_deferred =
            !self.cartridge.has_rtc() && self.cartridge.backup_kind() != BackupKind::Eeprom;
        self.frame_service_horizon = 0;
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
        if pending < u64::from(self.frame_service_horizon) {
            self.frame_service_pending_cycles = pending as u32;
            #[cfg(feature = "profiling")]
            {
                self.profiling.deferred_step_calls =
                    self.profiling.deferred_step_calls.wrapping_add(1);
                self.profiling.deferred_cycles = self
                    .profiling
                    .deferred_cycles
                    .wrapping_add(u64::from(cycles));
            }
            #[cfg(test)]
            {
                self.frame_service_deferred_calls =
                    self.frame_service_deferred_calls.wrapping_add(1);
            }
            return true;
        }
        // Keep the original observation-bearing chunk intact.
        #[cfg(feature = "profiling")]
        if self.frame_service_horizon_is_apu_observation() {
            self.profiling.apu_deadline_materializations =
                self.profiling.apu_deadline_materializations.wrapping_add(1);
        }
        self.materialize_frame_service();
        false
    }

    #[inline(never)]
    fn fresh_frame_service_horizon(&mut self) -> u32 {
        let deadline = if self.event_deadline.remaining == 0 {
            self.fresh_event_deadline()
        } else {
            self.event_deadline
        };
        let irq_sample = self
            .irq_delay_cycles
            .map_or(u32::MAX, |delay| delay.saturating_sub(3).max(1));
        deadline
            .remaining
            .min(irq_sample)
            .min(
                self.apu
                    .cycles_until_observation(super::read_io16(&self.io, super::SOUNDBIAS)),
            )
            .max(1)
    }

    pub(crate) fn materialize_frame_service(&mut self) {
        self.frame_service_observed = true;
        self.frame_service_horizon = 0;
        let pending = std::mem::take(&mut self.frame_service_pending_cycles);
        if pending != 0 {
            #[cfg(feature = "profiling")]
            self.profile_frame_service_pending_span(pending);
            self.step_cycles_eager(pending);
        }
    }

    pub(crate) fn frame_service_active(&self) -> bool {
        self.frame_service_deferred
    }

    pub(crate) fn frame_cpu_cycle_budget(&mut self) -> u32 {
        if !self.frame_service_deferred || self.pending_dma_cycles != 0 || self.ppu.frame_ready {
            return 0;
        }
        if self.frame_service_horizon == 0 {
            self.frame_service_horizon = self.fresh_frame_service_horizon();
        }
        self.frame_service_horizon
            .saturating_sub(self.frame_service_pending_cycles)
            .saturating_sub(1)
    }

    pub(crate) fn begin_frame_cpu_run(&mut self) {
        self.frame_service_observed = self.interrupt_ready();
    }

    #[inline]
    pub(crate) fn frame_cpu_run_can_continue(&self) -> bool {
        !self.frame_service_observed && self.pending_dma_cycles == 0 && !self.ppu.frame_ready
    }

    #[cfg(test)]
    pub(crate) fn frame_service_stats_for_test(&self) -> (bool, u32, u64) {
        (
            self.frame_service_deferred,
            self.frame_service_pending_cycles,
            self.frame_service_deferred_calls,
        )
    }
}

#[cfg(feature = "profiling")]
impl Bus {
    fn frame_service_horizon_is_apu_observation(&mut self) -> bool {
        let deadline = if self.event_deadline.remaining == 0 {
            self.fresh_event_deadline()
        } else {
            self.event_deadline
        };
        let irq_sample = self
            .irq_delay_cycles
            .map_or(u32::MAX, |delay| delay.saturating_sub(3).max(1));
        let non_apu_horizon = deadline.remaining.min(irq_sample);
        self.apu
            .cycles_until_observation(super::read_io16(&self.io, super::SOUNDBIAS))
            <= non_apu_horizon
    }

    fn profile_frame_service_pending_span(&mut self, pending: u32) {
        let profiling = &mut self.profiling;
        profiling.frame_service_pending_spans =
            profiling.frame_service_pending_spans.wrapping_add(1);
        profiling.frame_service_pending_cycles = profiling
            .frame_service_pending_cycles
            .wrapping_add(u64::from(pending));
        profiling.frame_service_pending_max_cycles =
            profiling.frame_service_pending_max_cycles.max(pending);
        let bucket = usize::try_from(pending.ilog2()).unwrap_or(0).min(7);
        profiling.frame_service_pending_span_buckets[bucket] =
            profiling.frame_service_pending_span_buckets[bucket].wrapping_add(1);
    }
}

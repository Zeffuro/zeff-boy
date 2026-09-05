use super::{Bus, DEADLINE_PPU, DEADLINE_TIMERS, SOUNDBIAS, read_io16};
use crate::hardware::timer::{
    Timer, TimerIrqExtraDelays, TimerOverflowCounts, TimerTimingState, Timers,
};

impl Bus {
    fn projected_timers_at(&self, cycles: u64) -> Timers {
        let mut timers = self.timers;
        let elapsed = cycles.wrapping_sub(self.timer_materialized_cycles);
        debug_assert!(elapsed <= u64::from(u32::MAX));
        let (interrupts, overflows, _) = timers.step_with_overflows(elapsed as u32);
        debug_assert_eq!(interrupts, 0);
        debug_assert_eq!(overflows, [0; 4]);
        timers
    }

    pub(super) fn timer_read16(&self, index: usize, control: bool) -> u16 {
        self.projected_timers_at(self.timer_observation_cycles)
            .read16(index, control)
    }

    pub(super) fn timer_cycles_until_overflow(&self, index: usize) -> Option<u32> {
        let elapsed = self
            .master_cycles
            .wrapping_sub(self.timer_materialized_cycles);
        debug_assert!(elapsed <= u64::from(u32::MAX));
        let elapsed = elapsed as u32;
        let deadline = self.timers.cycles_until_overflow(index)?;
        debug_assert!(elapsed < deadline);
        Some(deadline.saturating_sub(elapsed))
    }

    pub(super) fn materialize_timers_to(
        &mut self,
        cycles: u64,
    ) -> (u16, TimerOverflowCounts, TimerIrqExtraDelays) {
        let elapsed = cycles.wrapping_sub(self.timer_materialized_cycles);
        debug_assert!(elapsed <= u64::from(u32::MAX));
        if elapsed == 0 {
            return (0, [0; 4], [0; 4]);
        }
        self.timer_materialized_cycles = cycles;
        self.timers.step_with_overflows(elapsed as u32)
    }

    pub(super) fn materialize_timers_before_io_write(&mut self) {
        let (interrupts, overflows, _) = self.materialize_timers_to(self.timer_observation_cycles);
        debug_assert_eq!(interrupts, 0);
        debug_assert_eq!(overflows, [0; 4]);
    }

    pub(crate) fn timer_registers_snapshot(&self) -> [Timer; 4] {
        self.projected_timers_at(self.master_cycles).all()
    }

    pub(crate) fn timer_timing_state(&self) -> TimerTimingState {
        self.projected_timers_at(self.master_cycles).timing_state()
    }

    pub(crate) fn restore_master_cycles_after_state_load(&mut self, cycles: u64) {
        self.master_cycles = cycles;
        self.timer_materialized_cycles = cycles;
        self.timer_observation_cycles = cycles;
        self.invalidate_event_deadline_after_state_load();
    }

    #[cfg(test)]
    pub(crate) fn set_eager_timer_materialization_for_test(&mut self, eager: bool) {
        let (interrupts, overflows, _) = self.materialize_timers_to(self.master_cycles);
        assert_eq!(interrupts, 0);
        assert_eq!(overflows, [0; 4]);
        self.eager_timer_materialization = eager;
    }

    #[cfg(test)]
    pub(crate) fn timer_materialization_is_pending_for_test(&self) -> bool {
        self.timer_materialized_cycles != self.master_cycles
    }

    pub fn step_cycles(&mut self, mut cycles: u32) {
        #[cfg(feature = "profiling")]
        {
            self.profiling.step_calls = self.profiling.step_calls.wrapping_add(1);
            self.profiling.requested_cycles = self
                .profiling
                .requested_cycles
                .wrapping_add(u64::from(cycles));
        }
        while cycles > 0 {
            #[cfg(feature = "profiling")]
            {
                self.profiling.chunks = self.profiling.chunks.wrapping_add(1);
            }
            let soundcnt_h = read_io16(&self.io, 0x82);
            if self.event_deadline.remaining == 0 {
                self.event_deadline = self.fresh_event_deadline();
                #[cfg(feature = "profiling")]
                {
                    self.profiling.deadline_recomputes =
                        self.profiling.deadline_recomputes.wrapping_add(1);
                }
            } else {
                #[cfg(feature = "profiling")]
                {
                    self.profiling.deadline_hits = self.profiling.deadline_hits.wrapping_add(1);
                    assert_eq!(self.event_deadline, self.fresh_event_deadline());
                }
            }

            let deadline = self.event_deadline;
            let step = cycles.min(deadline.remaining);
            let due_sources = if step == deadline.remaining {
                deadline.sources
            } else {
                0
            };
            let ppu_before = (due_sources & DEADLINE_PPU != 0).then(|| {
                (
                    self.ppu.in_vblank(),
                    self.ppu.in_hblank(),
                    self.ppu.vcount(),
                )
            });
            self.event_deadline.remaining -= step;
            let chunk_start_cycles = self.master_cycles;
            self.master_cycles = self.master_cycles.wrapping_add(u64::from(step));
            self.timer_observation_cycles = self.master_cycles;

            #[cfg(feature = "profiling")]
            if due_sources != 0 {
                for source in 0..6 {
                    if due_sources & (1 << source) != 0 {
                        self.profiling.deadline_expiries[source] =
                            self.profiling.deadline_expiries[source].wrapping_add(1);
                    }
                }
            }

            self.step_irq_event(step);
            self.ppu.step_cycles(step);
            self.cartridge.step_cycles(step);
            cycles -= step;

            if ppu_before.is_some() {
                let (interrupts, overflows, _) = self.materialize_timers_to(chunk_start_cycles);
                debug_assert_eq!(interrupts, 0);
                debug_assert_eq!(overflows, [0; 4]);
                self.timer_observation_cycles = chunk_start_cycles;
            }

            if ppu_before.is_some_and(|(_, was_in_hblank, _)| {
                !was_in_hblank && self.ppu.in_hblank() && self.ppu.in_visible_scanline()
            }) {
                #[cfg(feature = "profiling")]
                {
                    self.profiling.visible_hblank_events =
                        self.profiling.visible_hblank_events.wrapping_add(1);
                    self.profiling.rendered_scanlines =
                        self.profiling.rendered_scanlines.wrapping_add(1);
                }
                self.ppu.render_current_scanline(
                    &self.io,
                    &self.palette_ram,
                    &self.vram,
                    &self.oam,
                );
                self.run_dma_start_timing(2);
            }
            if ppu_before
                .is_some_and(|(was_in_vblank, _, _)| !was_in_vblank && self.ppu.in_vblank())
            {
                #[cfg(feature = "profiling")]
                {
                    self.profiling.vblank_events = self.profiling.vblank_events.wrapping_add(1);
                }
                self.ppu.mark_frame_ready();
                self.run_dma_start_timing(1);
            }
            if let Some((was_in_vblank, was_in_hblank, old_vcount)) = ppu_before {
                self.update_lcd_interrupts(was_in_vblank, was_in_hblank, old_vcount);
            }
            self.timer_observation_cycles = self.master_cycles;
            self.apu.step_output(
                step,
                soundcnt_h,
                read_io16(&self.io, 0x84),
                read_io16(&self.io, SOUNDBIAS),
            );
            let materialize_timers = due_sources & (DEADLINE_PPU | DEADLINE_TIMERS) != 0;
            #[cfg(test)]
            let materialize_timers = materialize_timers || self.eager_timer_materialization;
            let (timer_interrupts, timer_overflows, timer_irq_extra_delays) = if materialize_timers
            {
                self.materialize_timers_to(self.master_cycles)
            } else {
                (0, [0; 4], [0; 4])
            };
            #[cfg(feature = "profiling")]
            for (total, count) in self
                .profiling
                .timer_overflows
                .iter_mut()
                .zip(timer_overflows)
            {
                *total = total.wrapping_add(u64::from(count));
            }
            if timer_overflows.iter().any(|&count| count != 0) {
                self.service_sound_timer_overflows(timer_overflows, soundcnt_h);
            }
            if timer_interrupts != 0 {
                self.request_timer_interrupts(timer_interrupts, timer_irq_extra_delays);
            }
        }
    }
}

use super::*;

impl BlankHistory {
    pub(in crate::hardware::vdp) fn fold_interrupt_forecast(
        &mut self,
        region: Region,
        registers: &mut VdpRegisterPipeline,
    ) -> Option<()> {
        let mut clock = self.vertical?;
        if self.port.is_some()
            || self.data_inflight.is_some()
            || registers.pending().is_some()
            || registers.now() != self.now
            || !self.vertical_supported(registers, region)
        {
            return None;
        }
        let dot = clock.dot();
        let phase = self.next_clock.get().checked_sub(clock.line_start.get())?;
        if self.next_clock <= self.now || !phase.is_multiple_of(dot) {
            return None;
        }
        let mode = registers.registers()[1]?;
        let hint = registers.registers()[10]?;
        let h = clock.counter(self.next_clock).ok()?;
        let mut probe = self.core;
        probe.commit_at(self.next_clock, h).ok()?;
        probe.settle(mode);
        if probe.b {
            probe.t6 = false;
        }
        probe.prepare_clock(region, registers, h, Some(clock.clock));
        if probe != self.core {
            return None;
        }
        let wide = clock.clock != super::super::clock::LineClock::H32;
        let clear = clock
            .line_start
            .get()
            .checked_add((if wide { 322 } else { 258 }) * dot)?;
        let select = clock
            .line_start
            .get()
            .checked_add((if wide { 328 } else { 264 }) * dot)?;
        let boundary = clock.boundary().ok()?.get();
        let next = [clear, select, boundary]
            .into_iter()
            .filter(|at| *at >= self.next_clock.get())
            .min()?;
        let last = next.checked_sub(dot)?;
        if last <= self.next_clock.get() {
            return None;
        }
        // The complete Core is a fixed point until the next horizontal decode.
        let last = MasterTicks::new(last);
        registers.advance_to(last).ok()?;
        self.now = last;
        self.core.updated_at = last;
        self.next_clock = MasterTicks::new(next);
        clock.prepared_at = last;
        clock.prepared_mode = mode;
        clock.prepared_hint = hint;
        self.vertical = Some(clock);
        Some(())
    }

    pub fn next_interrupt(
        self,
        after: MasterTicks,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<VdpInterruptEdge, VdpTimingError> {
        let mut view = *registers;
        let mut history = self.advance_projection(after, region, &mut view)?;
        loop {
            let _ = history.fold_interrupt_forecast(region, &mut view);
            let next = history.next_event(&view)?;
            if history.vertical.is_some() && history.port.is_some_and(|p| next > p.end) {
                return Err(VdpTimingError::ClockOverflow);
            }
            let before = history.core;
            history = history.advance_projection(next, region, &mut view)?;
            let vertical = history.core.h && history.core.q && !(before.h && before.q);
            if history.core.v != before.v || vertical {
                return Ok(VdpInterruptEdge {
                    at: next,
                    clock: if vertical {
                        VdpInterruptClock::Vertical
                    } else {
                        VdpInterruptClock::HorizontalCounter
                    },
                });
            }
        }
    }
}

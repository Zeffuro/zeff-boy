use super::*;

impl VdpInterrupts {
    pub(crate) fn scroll_fetch_phase(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<u64, VdpTimingError> {
        self.timing.scroll_fetch_phase(self.now, region, registers)
    }
    pub(crate) fn is_scroll_fetch(&self, region: Region, registers: &VdpRegisterPipeline) -> bool {
        self.timing.scroll_fetch_at(self.now, region, registers) == Ok(self.now)
    }

    pub(crate) fn next_scroll_deadline(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpTimingError> {
        self.timing
            .next_scroll_deadline(self.now, region, registers)
    }

    pub(crate) fn has_timing_history(&self) -> bool {
        self.timing.history.is_some()
    }

    pub(crate) fn effective_line_ticks(&self) -> u64 {
        if let Some(vertical) = self.timing.blank.and_then(|b| b.vertical) {
            return vertical.clock.ticks();
        }
        self.timing
            .history
            .map_or(H32_LINE_TICKS, |history| history.cursor.clock.ticks())
    }

    pub(crate) fn next_row_deadline(
        &self,
        _region: Region,
        _registers: &VdpRegisterPipeline,
    ) -> Result<MasterTicks, VdpTimingError> {
        self.timing.next_row_boundary(self.now)
    }

    pub(super) fn advance_anchored_pair(
        &mut self,
        at: MasterTicks,
        region: Region,
        registers: &mut VdpRegisterPipeline,
        require_display: bool,
    ) -> Result<Option<VdpEvent>, VdpScheduleError> {
        self.timing
            .elapsed_in_mode(self.now, region, registers, require_display)
            .map_err(VdpInterruptError::from)?;
        let mut candidate = *self;
        let mut projected = self
            .timing
            .anchored_projection(region, registers)
            .map_err(VdpInterruptError::from)?
            .unwrap();
        let mut result = None;
        loop {
            let next = projected.next_event().map_err(VdpInterruptError::from)?;
            if next > at {
                projected.advance_to(at).map_err(VdpInterruptError::from)?;
                break;
            }
            let before = projected;
            let event = projected.step().map_err(VdpInterruptError::from)?;
            if event.publication && (event.horizontal || event.vertical) && require_display {
                return Err(VdpScheduleError::CoincidentPublication(event.at));
            }
            if event.horizontal {
                let mut edge_view = before;
                edge_view.field.updated_at = event.at;
                let counter = edge_view.vcounter_now().map_err(VdpInterruptError::from)?;
                let reload = published(&before.registers, HINT_RELOAD_REGISTER)?;
                if usize::from(counter) <= before.registers.display_height(region) {
                    if let Some(value) = candidate.line_counter.checked_sub(1) {
                        candidate.line_counter = value;
                    } else {
                        candidate.line_counter = reload;
                        candidate.horizontal_pending = true;
                    }
                } else {
                    candidate.line_counter = reload;
                }
            }
            if event.vertical {
                candidate.vertical_pending = true;
            }
            if event.horizontal || event.vertical {
                result = Some(VdpEvent::Interrupt(VdpInterruptEdge {
                    at: event.at,
                    clock: if event.vertical {
                        VdpInterruptClock::Vertical
                    } else {
                        VdpInterruptClock::HorizontalCounter
                    },
                }));
            } else if event.publication {
                result = before.registers.pending().map(VdpEvent::Register);
            }
            if result.is_some() {
                break;
            }
        }
        candidate.now = projected.field.updated_at;
        candidate.timing.field = projected.field;
        if candidate.has_timing_history()
            || projected.registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]
                .is_some_and(|value| value & H40_CLOCK_MASK == 1)
        {
            candidate.timing.history = Some(projected.history);
        }
        *self = candidate;
        *registers = projected.registers;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;

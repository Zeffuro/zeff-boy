use super::*;

impl VdpInterrupts {
    pub(in crate::hardware::vdp) fn interval_end(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Option<MasterTicks> {
        let field = self.timing.field;
        if self.timing.blank.is_some()
            || self.timing.history.is_some()
            || registers.pending().is_some()
            || registers.now() != self.now
            || field.updated_at != self.now
            || field.mode.interlaced()
            || field.active_mode.interlaced()
            || field.odd
            || field.active_odd
            || self.now < field.start
        {
            return None;
        }
        for index in [
            HINT_ENABLE_REGISTER,
            MODE_REGISTER,
            HINT_RELOAD_REGISTER,
            DISPLAY_SIZE_REGISTER,
        ] {
            registers.registers()[usize::from(index)]?;
        }
        if registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)]? & INTERLACE_MODE_MASK != 0
            || !self
                .timing
                .shares_internal_phase(registers.published_state().0)
        {
            return None;
        }
        let end = field.end(region).ok()?;
        if self.now >= end {
            return None;
        }
        let edge = self
            .timing
            .next_counter_edge(self.now, region, registers, false)
            .ok()?;
        let end = end.min(edge.at);
        (end > self.now).then_some(end)
    }

    pub(in crate::hardware::vdp) fn advance_interval_clock(&mut self, at: MasterTicks) {
        debug_assert!(at > self.now);
        debug_assert_eq!(self.timing.field.updated_at, self.now);
        self.now = at;
        self.timing.field.updated_at = at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_admission_requires_settled_calendar_and_matching_clocks() {
        let registers = VdpRegisterPipeline::bootstrap_mode5();
        let interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
        assert!(
            interrupts
                .interval_end(Region::NtscUs, &registers)
                .is_some()
        );
        for flags in 0..4 {
            let mut candidate = interrupts;
            match flags {
                0 => candidate.timing.field.mode = crate::hardware::vdp::InterlaceMode::Double,
                1 => {
                    candidate.timing.field.active_mode = crate::hardware::vdp::InterlaceMode::Double
                }
                2 => candidate.timing.field.odd = true,
                _ => candidate.timing.field.active_odd = true,
            }
            assert!(candidate.interval_end(Region::NtscUs, &registers).is_none());
        }
        let mut candidate = interrupts;
        candidate.timing.field.updated_at = MasterTicks::new(1);
        assert!(candidate.interval_end(Region::NtscUs, &registers).is_none());
        candidate = interrupts;
        candidate.timing.field.start = MasterTicks::new(1);
        assert!(candidate.interval_end(Region::NtscUs, &registers).is_none());
        let mut later = registers;
        later.advance_to(MasterTicks::new(1)).unwrap();
        assert!(interrupts.interval_end(Region::NtscUs, &later).is_none());
    }

    #[test]
    fn interval_field_id_limit_ends_before_failing_rollover() {
        let mut registers = VdpRegisterPipeline::bootstrap_mode5();
        let mut interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
        interrupts.timing.field.id = u64::MAX;
        let end = interrupts.timing.field.end(Region::NtscUs).unwrap();
        let before = MasterTicks::new(end.get() - 1);
        while interrupts
            .advance_pair(before, Region::NtscUs, &mut registers, false)
            .unwrap()
            .is_some()
        {}
        assert_eq!(
            interrupts.interval_end(Region::NtscUs, &registers),
            Some(end)
        );
        let saved = (interrupts, registers);
        assert!(
            interrupts
                .advance_pair(end, Region::NtscUs, &mut registers, false)
                .is_err()
        );
        assert_eq!((interrupts, registers), saved);
    }
}

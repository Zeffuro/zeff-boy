use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SoundInterrupt {
    timing: VdpVerticalTiming,
    registers: super::super::timing::SoundRegisters,
    region: Region,
    blank: Option<VdpRegisterPipeline>,
}

impl VdpInterrupts {
    pub(crate) fn sound_interrupt(
        &self,
        registers: &VdpRegisterPipeline,
        region: Region,
    ) -> SoundInterrupt {
        SoundInterrupt {
            timing: self.timing,
            registers: super::super::timing::SoundRegisters::capture(registers),
            region,
            blank: self.timing.blank.map(|_| *registers),
        }
    }
}

impl SoundInterrupt {
    #[inline(never)]
    pub(crate) fn asserted_at(&self, now: u64) -> Result<bool, VdpTimingError> {
        let at = MasterTicks::new(now);
        if self
            .timing
            .blank
            .is_some_and(|blank| blank.port.is_some_and(|port| at > port.end))
        {
            return Err(VdpTimingError::UnqualifiedDisplayHistory);
        }
        if let (Some(blank), Some(mut registers)) = (self.timing.blank, self.blank) {
            let activates = registers.pending().is_some_and(|write| {
                write.publish_at <= at
                    && (blank.activates(&registers, write.publish_at)
                        || blank.activates_vertical(&registers, write.publish_at, self.region))
            });
            if blank.authoritative() || at >= blank.now && blank.eligible && activates {
                let projected = blank.advance_projection(at, self.region, &mut registers)?;
                if projected.authoritative() {
                    return Ok(projected.core.sound);
                }
            }
        }
        if self.timing.uses_history(&self.registers) {
            return self.anchored_asserted_at(at);
        }
        if at == self.timing.field.updated_at {
            return self.asserted_in_field(at, self.timing.field.start, &self.registers);
        }
        self.projected_asserted_at(at)
    }

    #[inline(never)]
    fn projected_asserted_at(&self, at: MasterTicks) -> Result<bool, VdpTimingError> {
        let (calendar, registers) =
            self.timing
                .sound_calendar_at(at, self.region, &self.registers)?;
        self.asserted_in_field(at, calendar.start, &registers)
    }

    fn asserted_in_field(
        &self,
        at: MasterTicks,
        field_start: MasterTicks,
        registers: &super::super::timing::SoundRegisters,
    ) -> Result<bool, VdpTimingError> {
        let mode = self.timing.sound_mode_at(at, self.region, registers)?;
        let start = registers.display_height(self.region) as u64 * H32_LINE_TICKS
            + mode.vertical_interrupt();
        let phase = at.get() - field_start.get();
        Ok((start..start + H32_LINE_TICKS).contains(&phase))
    }

    #[inline(never)]
    fn anchored_asserted_at(&self, at: MasterTicks) -> Result<bool, VdpTimingError> {
        let projected = self
            .timing
            .anchored_projection(self.region, &self.registers)?
            .unwrap();
        Ok(projected.project(at)?.sound_now())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod current_tests;

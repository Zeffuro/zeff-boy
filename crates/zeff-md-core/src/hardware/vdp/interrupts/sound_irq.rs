use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SoundInterrupt {
    timing: VdpVerticalTiming,
    registers: VdpRegisterPipeline,
    region: Region,
}

impl VdpInterrupts {
    pub(crate) fn sound_interrupt(
        &self,
        registers: VdpRegisterPipeline,
        region: Region,
    ) -> SoundInterrupt {
        SoundInterrupt {
            timing: self.timing,
            registers,
            region,
        }
    }
}

impl SoundInterrupt {
    pub(crate) fn asserted_at(self, now: u64) -> Result<bool, VdpTimingError> {
        let at = MasterTicks::new(now);
        let (calendar, registers) =
            self.timing
                .sound_calendar_at(at, self.region, &self.registers)?;
        let registers = registers.published_state().1;
        self.timing
            .elapsed_in_mode(at, self.region, &registers, false)?;
        let mode = registers
            .horizontal_mode()
            .expect("validated horizontal mode");
        let start = registers.display_height(self.region) as u64 * H32_LINE_TICKS
            + mode.vertical_interrupt();
        let phase = now - calendar.start.get();
        Ok((start..start + H32_LINE_TICKS).contains(&phase))
    }
}

#[cfg(test)]
mod tests;

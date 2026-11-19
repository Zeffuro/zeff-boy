use super::*;

impl<C: SoundCapture> ScheduledBus<'_, C> {
    pub(super) fn wait_svp_main(&mut self, waits: &mut u32) -> Result<(), BusError> {
        while let Some(clocks) = self.bus.svp_main_wait()? {
            let total = waits.checked_add(clocks).ok_or(BusError::ClockOverflow)?;
            self.advance_main(self.bus.cpu_clock, clocks, false)?;
            *waits = total;
        }
        Ok(())
    }
}

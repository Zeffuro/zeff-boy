use super::*;

impl ScheduledBus<'_> {
    pub(crate) fn advance_main_cycle_reference(
        &mut self,
        start: CpuClocks,
        clocks: u32,
        occupied: bool,
        acknowledge: bool,
    ) -> Result<(), BusError> {
        let begin = self.bus.now.get();
        let sound_irq = self.bus.sound_interrupt();
        let external = self.bus.external_dma_wait()?.map(|(_, release)| release);
        let sound_active = !self.bus.sound.reset_asserted && !self.bus.sound.granted;
        self.bus.advance_main_cycle(start, clocks, acknowledge)?;
        if clocks != 0 && sound_active {
            self.bus.prefetch = Prefetch::Empty;
        }
        self.advance_sound(begin, occupied, external, &sound_irq);
        Ok(())
    }

    pub(crate) fn advance_main_cycle_candidate(
        &mut self,
        start: CpuClocks,
        clocks: u32,
        occupied: bool,
        acknowledge: bool,
    ) -> Result<(), BusError> {
        self.advance_main_cycle(start, clocks, occupied, acknowledge)
    }
}

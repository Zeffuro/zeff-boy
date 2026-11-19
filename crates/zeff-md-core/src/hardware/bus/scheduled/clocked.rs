use super::*;

impl<C: SoundCapture> ClockedBus for ScheduledBus<'_, C> {
    type Error = BusError;
    fn read_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: zeff_m68k::timing::InterruptAperture,
    ) -> zeff_m68k::timing::ObservedCycle<u16, Self::Error> {
        self.read_observed_with(access, start, aperture, std::convert::identity)
    }
    fn read_instruction_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, BusError> {
        self.read_input_cycle_with(access, start, std::convert::identity)
    }
    fn read_instruction_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: zeff_m68k::timing::InterruptAperture,
    ) -> zeff_m68k::timing::ObservedCycle<InstructionInput, BusError> {
        self.read_input_observed_with(access, start, aperture, std::convert::identity)
    }
    fn write_cycle_observed(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> zeff_m68k::timing::ObservedCycle<(), Self::Error> {
        self.write_observed_with(access, value, start, std::convert::identity)
    }
    fn write_cycle_with_previous(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> (CycleResult<(), Self::Error>, [Option<u8>; 2]) {
        let mut previous = previous::Previous::default();
        let cycle =
            self.write_cycle_capturing(access, value, start, std::convert::identity, &mut previous);
        let previous = previous.finish(cycle.result.is_ok());
        (cycle, previous)
    }
    fn write_cycle_observed_with_previous(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> (
        zeff_m68k::timing::ObservedCycle<(), Self::Error>,
        [Option<u8>; 2],
    ) {
        let mut previous = previous::Previous::default();
        let cycle = self.write_observed_with_capturing(
            access,
            value,
            start,
            std::convert::identity,
            &mut previous,
        );
        let previous = previous.finish(cycle.cycle.result.is_ok());
        (cycle, previous)
    }
    fn read_discarded_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        self.read_discarded_cycle_with(access, start, std::convert::identity)
    }
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        self.read_cycle_with(access, start, std::convert::identity)
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        self.write_cycle_with(access, value, start, std::convert::identity)
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        if self.bus.rmw.is_some() {
            if self.idle_ram_rmw(start, clocks).is_err() {
                self.bus.clock_fault = true;
            }
            return;
        }
        if self.advance_main(start, clocks, false).is_err() {
            self.bus.clock_fault = true;
        } else if clocks != 0 {
            self.bus.prefetch = Prefetch::Empty;
        }
    }
    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, BusError> {
        self.acknowledge_cycle_with(access, start, std::convert::identity)
    }
    fn begin_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, BusError> {
        self.bus.check_main_clock(start)?;
        if self.schedule.rmw_checkpoint.is_some() {
            return Ok(BusResponse::Unsupported);
        }
        self.bus.begin_ram_rmw(access, start)
    }
    fn end_rmw(&mut self, _end: CpuClocks) {
        self.end_ram_rmw();
    }
    fn begin_reset_output(&mut self, start: CpuClocks) -> Result<BusResponse<()>, BusError> {
        if self
            .bus
            .cartridge
            .registers
            .as_ref()
            .is_some_and(|board| board.is_banked())
            || self.bus.cartridge.parallel_adapter.is_some()
            || self.bus.cartridge.sf004.is_some()
            || self.bus.cartridge.extended_ssf.is_some()
            || self.bus.cartridge.realtec.is_some()
            || self.bus.cartridge.svp.is_some()
        {
            return Ok(BusResponse::Unsupported);
        }
        if self.bus.input.uart_reset_blocked() {
            return Ok(BusResponse::Unsupported);
        }
        if self
            .bus
            .cartridge
            .sf002
            .as_ref()
            .is_some_and(|board| board.validate_guest_reset_output().is_err())
        {
            return Ok(BusResponse::Unsupported);
        }
        let epoch_access = self.begin_epoch_access(false);
        let result = self.bus.begin_reset_output(start);
        self.end_epoch_access(epoch_access);
        result
    }
    fn end_reset_output(&mut self, end: CpuClocks) {
        let epoch_access = self.begin_epoch_access(false);
        self.bus.end_reset_output(end);
        self.end_epoch_access(epoch_access);
    }
}

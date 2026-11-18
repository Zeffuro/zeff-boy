use super::*;

impl ClockedBus for Bus {
    type Error = BusError;
    fn read_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: zeff_m68k::timing::InterruptAperture,
    ) -> zeff_m68k::timing::ObservedCycle<u16, Self::Error> {
        self.read_observed(access, start, aperture)
    }
    fn write_cycle_observed(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> zeff_m68k::timing::ObservedCycle<(), Self::Error> {
        self.write_observed(access, value, start)
    }
    fn write_cycle_with_previous(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> (CycleResult<(), Self::Error>, [Option<u8>; 2]) {
        let mut previous = previous::Previous::default();
        let cycle = self.write_cycle_capturing(access, value, start, &mut previous);
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
        let cycle = self.write_observed_capturing(access, value, start, &mut previous);
        let previous = previous.finish(cycle.cycle.result.is_ok());
        (cycle, previous)
    }
    fn read_discarded_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        if !self.discarded_read(access) || self.rmw.is_some() {
            let cycle = self.read_cycle(access, start);
            return CycleResult {
                result: cycle.result.map(|_| ()),
                wait_clocks: cycle.wait_clocks,
            };
        }
        let mut waits = 0;
        let result = (|| {
            self.check_main_clock(start)?;
            self.wait_external_dma(&mut waits)?;
            self.advance_main(self.cpu_clock, TRANSFER_CLOCKS)?;
            self.complete_discarded_read(access);
            Ok(())
        })();
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        instruction_input::value(self.read_input_cycle_timed(access, start))
    }

    fn read_instruction_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, BusError> {
        self.read_input_cycle_timed(access, start)
    }

    fn read_instruction_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: zeff_m68k::timing::InterruptAperture,
    ) -> zeff_m68k::timing::ObservedCycle<InstructionInput, BusError> {
        self.read_input_observed(access, start, aperture)
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        self.write_cycle_capturing(access, value, start, &mut ())
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        if self.rmw.is_some() {
            if self.idle_ram_rmw(start, clocks).is_err() {
                self.clock_fault = true;
            }
            return;
        }
        if self.advance_main(start, clocks).is_err() {
            self.clock_fault = true;
        } else if clocks != 0 {
            self.prefetch = Prefetch::Empty;
        }
    }
    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, BusError> {
        let mut waits = 0;
        let result = (|| {
            self.reject_rmw_transfer(access)?;
            self.check_main_clock(start)?;
            self.wait_external_dma(&mut waits)?;
            self.advance_main_cycle(self.cpu_clock, TRANSFER_CLOCKS, true)?;
            self.audio.advance_to(self.now.get());
            self.prefetch = Prefetch::Empty;
            Ok(BusResponse::Complete(InterruptVector::Autovector))
        })();
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }
    fn begin_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, BusError> {
        self.begin_ram_rmw(access, start)
    }
    fn end_rmw(&mut self, _end: CpuClocks) {
        self.rmw = None;
    }
    fn begin_reset_output(&mut self, _: CpuClocks) -> Result<BusResponse<()>, BusError> {
        if self
            .cartridge
            .registers
            .as_ref()
            .is_some_and(|board| board.is_banked())
            || self.cartridge.parallel_adapter.is_some()
            || self.cartridge.sf004.is_some()
            || self.cartridge.extended_ssf.is_some()
            || self.cartridge.realtec.is_some()
            || self.cartridge.svp.is_some()
        {
            return Ok(BusResponse::Unsupported);
        }
        if self.input.uart_reset_blocked() {
            return Ok(BusResponse::Unsupported);
        }
        if self
            .cartridge
            .sf002
            .as_ref()
            .is_some_and(|board| board.validate_guest_reset_output().is_err())
            || self.rmw.is_some()
        {
            return Ok(BusResponse::Unsupported);
        }
        self.prefetch = Prefetch::Empty;
        self.input.retire_uart_reset();
        self.vdp.retire_timed_port_history();
        self.reset_output = true;
        Ok(BusResponse::Complete(()))
    }
    fn end_reset_output(&mut self, _: CpuClocks) {
        self.reset_output = false;
        self.reset_pulses = self.reset_pulses.wrapping_add(1);
    }
}

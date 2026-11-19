use super::*;
use zeff_m68k::timing::TimingFault;

pub(crate) struct CompactBus<'a, C = NoSoundCapture>(pub ScheduledBus<'a, C>);

#[cold]
#[inline(never)]
fn boxed(error: BusError) -> Box<BusError> {
    Box::new(error)
}

impl<C: SoundCapture> ClockedBus for CompactBus<'_, C> {
    type Error = Box<BusError>;
    fn read_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: zeff_m68k::timing::InterruptAperture,
    ) -> zeff_m68k::timing::ObservedCycle<u16, Self::Error> {
        self.0.read_observed_with(access, start, aperture, boxed)
    }
    fn read_instruction_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, Self::Error> {
        self.0.read_input_cycle_with(access, start, boxed)
    }
    fn read_instruction_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: zeff_m68k::timing::InterruptAperture,
    ) -> zeff_m68k::timing::ObservedCycle<InstructionInput, Self::Error> {
        self.0
            .read_input_observed_with(access, start, aperture, boxed)
    }
    fn write_cycle_observed(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> zeff_m68k::timing::ObservedCycle<(), Self::Error> {
        self.0.write_observed_with(access, value, start, boxed)
    }

    fn write_cycle_with_previous(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> (CycleResult<(), Self::Error>, [Option<u8>; 2]) {
        let mut previous = previous::Previous::default();
        let cycle = self
            .0
            .write_cycle_capturing(access, value, start, boxed, &mut previous);
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
        let cycle =
            self.0
                .write_observed_with_capturing(access, value, start, boxed, &mut previous);
        let previous = previous.finish(cycle.cycle.result.is_ok());
        (cycle, previous)
    }
    fn read_discarded_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        self.0.read_discarded_cycle_with(access, start, boxed)
    }

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, Self::Error> {
        self.0.read_cycle_with(access, start, boxed)
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        self.0.write_cycle_with(access, value, start, boxed)
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.0.idle(start, clocks);
    }

    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, Self::Error> {
        self.0.acknowledge_cycle_with(access, start, boxed)
    }

    fn begin_reset_output(&mut self, start: CpuClocks) -> Result<BusResponse<()>, Self::Error> {
        self.0.begin_reset_output(start).map_err(boxed)
    }

    fn end_reset_output(&mut self, end: CpuClocks) {
        self.0.end_reset_output(end);
    }

    fn begin_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, Self::Error> {
        self.0.begin_rmw(access, start).map_err(boxed)
    }

    fn end_rmw(&mut self, end: CpuClocks) {
        self.0.end_rmw(end);
    }
}

pub(crate) fn expand_fault(fault: TimingFault<Box<BusError>>) -> TimingFault<BusError> {
    match fault {
        TimingFault::AddressError { address } => TimingFault::AddressError { address },
        TimingFault::Bus {
            access,
            direction,
            source,
        } => TimingFault::Bus {
            access,
            direction,
            source: *source,
        },
        TimingFault::UnsupportedInstruction { pc, opcode } => {
            TimingFault::UnsupportedInstruction { pc, opcode }
        }
        TimingFault::DebugCallRejected(reason) => TimingFault::DebugCallRejected(reason),
        TimingFault::Unprimed => TimingFault::Unprimed,
        TimingFault::Frozen => TimingFault::Frozen,
        TimingFault::ClockOverflow => TimingFault::ClockOverflow,
        TimingFault::InterruptAcknowledgeUnsupported => {
            TimingFault::InterruptAcknowledgeUnsupported
        }
        TimingFault::ResetOutputUnsupported => TimingFault::ResetOutputUnsupported,
        TimingFault::ResetOutput { source } => TimingFault::ResetOutput { source: *source },
        TimingFault::RmwOwnershipUnsupported { access } => {
            TimingFault::RmwOwnershipUnsupported { access }
        }
        TimingFault::RmwOwnership { access, source } => TimingFault::RmwOwnership {
            access,
            source: *source,
        },
        TimingFault::Stopped => TimingFault::Stopped,
        TimingFault::PendingTrace => TimingFault::PendingTrace,
        TimingFault::DoubleFaultHalted => TimingFault::DoubleFaultHalted,
        TimingFault::ImportedBoundaryPending => TimingFault::ImportedBoundaryPending,
        TimingFault::PhysicalFaultUnqualified => TimingFault::PhysicalFaultUnqualified,
    }
}

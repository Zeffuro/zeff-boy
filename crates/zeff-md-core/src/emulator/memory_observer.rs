use super::cpu_operation::CpuOperation;
use zeff_m68k::bus::{AccessKind, BusAccess, BusResponse, ByteLanes, Direction};
use zeff_m68k::timing::{
    ClockedBus, CpuClocks, CycleResult, InstructionInput, InterruptAperture, InterruptVector,
    ObservedCycle, PrefetchCpu, TimedOutcome, TimingFault,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryAccess {
    pub address: u32,
    /// Discarded read cycles do not expose their sampled value.
    pub value: Option<u8>,
    pub old_value: Option<u8>,
    pub direction: Direction,
}

pub trait InstructionObserver {
    fn needs_previous_values(&self) -> bool {
        true
    }
    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        self.instruction_word(address, input.value);
    }
    fn instruction_word(&mut self, _address: u32, _value: u16) {}
    fn memory_access(&mut self, _access: MemoryAccess) {}
    fn write(&mut self, _access: BusAccess, _value: u16) {}
    fn write_with_previous(&mut self, access: BusAccess, value: u16, _old: [Option<u8>; 2]) {
        self.write(access, value);
    }
    fn cpu_boundary(&mut self, _outcome: TimedOutcome) {}
}

impl<F: FnMut(MemoryAccess)> InstructionObserver for F {
    fn memory_access(&mut self, access: MemoryAccess) {
        self(access);
    }
}

pub(super) struct Direct;

impl CpuOperation for Direct {
    type Output = TimedOutcome;

    fn step<B: ClockedBus>(
        &mut self,
        cpu: &mut PrefetchCpu,
        bus: &mut B,
    ) -> Result<Self::Output, TimingFault<B::Error>> {
        cpu.step_boundary(bus)
    }

    fn stopped(output: &Self::Output) -> bool {
        *output == TimedOutcome::Stopped
    }
}

pub(super) struct Observer<'a, F>(pub &'a mut F);

impl<F: InstructionObserver> CpuOperation for Observer<'_, F> {
    type Output = TimedOutcome;

    fn step<B: ClockedBus>(
        &mut self,
        cpu: &mut PrefetchCpu,
        bus: &mut B,
    ) -> Result<Self::Output, TimingFault<B::Error>> {
        let result = cpu.step_boundary(&mut ObservedBus {
            inner: bus,
            observer: self.0,
        });
        if let Ok(outcome) = &result {
            self.0.cpu_boundary(*outcome);
        }
        result
    }

    fn stopped(output: &Self::Output) -> bool {
        *output == TimedOutcome::Stopped
    }
}

struct ObservedBus<'a, B, F> {
    inner: &'a mut B,
    observer: &'a mut F,
}

impl<B, F: InstructionObserver> ObservedBus<'_, B, F> {
    fn observe(
        &mut self,
        access: BusAccess,
        value: Option<u16>,
        direction: Direction,
        old: [Option<u8>; 2],
    ) {
        if !matches!(access.kind, AccessKind::Data | AccessKind::Stack) {
            return;
        }
        if access.lanes != ByteLanes::Lower {
            self.observer.memory_access(MemoryAccess {
                address: access.bus_address,
                value: value.map(|value| (value >> 8) as u8),
                old_value: old[0],
                direction,
            });
        }
        if access.lanes != ByteLanes::Upper {
            self.observer.memory_access(MemoryAccess {
                address: access.bus_address | 1,
                value: value.map(|value| value as u8),
                old_value: old[1],
                direction,
            });
        }
    }
}

impl<B: ClockedBus, F: InstructionObserver> ClockedBus for ObservedBus<'_, B, F> {
    type Error = B::Error;

    fn instruction_word(&mut self, address: u32, value: u16) {
        self.observer.instruction_word(address, value);
        self.inner.instruction_word(address, value);
    }

    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        self.observer.instruction_input(address, input);
        self.inner.instruction_input(address, input);
    }

    fn read_instruction_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, B::Error> {
        let cycle = self.inner.read_instruction_cycle(access, start);
        if let Ok(input) = cycle.result {
            self.observe(access, Some(input.value), Direction::Read, [None, None]);
        }
        cycle
    }

    fn read_instruction_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: InterruptAperture,
    ) -> ObservedCycle<InstructionInput, B::Error> {
        let observed = self
            .inner
            .read_instruction_cycle_observed(access, start, aperture);
        if let Ok(input) = observed.cycle.result {
            self.observe(access, Some(input.value), Direction::Read, [None, None]);
        }
        observed
    }

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, B::Error> {
        let cycle = self.inner.read_cycle(access, start);
        if let Ok(value) = cycle.result {
            self.observe(access, Some(value), Direction::Read, [None, None]);
        }
        cycle
    }

    fn read_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: InterruptAperture,
    ) -> ObservedCycle<u16, B::Error> {
        let observed = self.inner.read_cycle_observed(access, start, aperture);
        if let Ok(value) = observed.cycle.result {
            self.observe(access, Some(value), Direction::Read, [None, None]);
        }
        observed
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), B::Error> {
        let (cycle, previous) = if self.observer.needs_previous_values() {
            self.inner.write_cycle_with_previous(access, value, start)
        } else {
            (self.inner.write_cycle(access, value, start), [None, None])
        };
        if cycle.result.is_ok() {
            if matches!(access.kind, AccessKind::Data | AccessKind::Stack) {
                self.observer.write_with_previous(access, value, previous);
            }
            self.observe(access, Some(value), Direction::Write, previous);
        }
        cycle
    }

    fn write_cycle_observed(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> ObservedCycle<(), B::Error> {
        let (observed, previous) = if self.observer.needs_previous_values() {
            self.inner
                .write_cycle_observed_with_previous(access, value, start)
        } else {
            (
                self.inner.write_cycle_observed(access, value, start),
                [None, None],
            )
        };
        if observed.cycle.result.is_ok() {
            if matches!(access.kind, AccessKind::Data | AccessKind::Stack) {
                self.observer.write_with_previous(access, value, previous);
            }
            self.observe(access, Some(value), Direction::Write, previous);
        }
        observed
    }

    fn read_discarded_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<(), B::Error> {
        let cycle = self.inner.read_discarded_cycle(access, start);
        if cycle.result.is_ok() {
            self.observe(access, None, Direction::Read, [None, None]);
        }
        cycle
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.inner.idle(start, clocks);
    }

    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, B::Error> {
        self.inner.acknowledge_cycle(access, start)
    }

    fn begin_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, B::Error> {
        self.inner.begin_rmw(access, start)
    }

    fn end_rmw(&mut self, end: CpuClocks) {
        self.inner.end_rmw(end);
    }

    fn begin_reset_output(&mut self, start: CpuClocks) -> Result<BusResponse<()>, B::Error> {
        self.inner.begin_reset_output(start)
    }

    fn end_reset_output(&mut self, end: CpuClocks) {
        self.inner.end_reset_output(end);
    }
}

#[cfg(test)]
mod tests;

use super::context::TraceContext;
use super::{BusRecord, Callback, Outcome, TraceError, begin_call, enabled, end_call};
use zeff_m68k::bus::{BusAccess, BusResponse};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, InstructionInput, InterruptVector};

pub(crate) struct TraceBus<'a, B> {
    inner: &'a mut B,
}

impl<'a, B> TraceBus<'a, B> {
    pub(crate) fn new(inner: &'a mut B) -> Self {
        Self { inner }
    }
}

impl<B: TraceContext> TraceBus<'_, B> {
    fn begin(
        &self,
        callback: Callback,
        access: Option<BusAccess>,
        start: CpuClocks,
        value: Option<u16>,
        idle_clocks: u32,
    ) -> Option<u64> {
        if !enabled() {
            return None;
        }
        begin_call(BusRecord {
            sequence: 0,
            boundary: 0,
            callback,
            access,
            write_value: value,
            requested_start: start.0,
            idle_clocks,
            wait_clocks: 0,
            entry: self
                .inner
                .trace_context(access.map(|access| access.bus_address)),
            service: None,
            vdp_control: None,
            returned: None,
            outcome: None,
        })
    }

    fn end(&self, token: Option<u64>, access: Option<BusAccess>, waits: u32, outcome: Outcome) {
        if let Some(sequence) = token {
            end_call(
                sequence,
                self.inner
                    .trace_context(access.map(|access| access.bus_address)),
                waits,
                outcome,
            );
        }
    }
}

pub(super) fn unit_outcome<E: TraceError>(result: &Result<BusResponse<()>, E>) -> Outcome {
    match result {
        Ok(BusResponse::Complete(())) => Outcome::Complete,
        Ok(BusResponse::Unsupported) => Outcome::Unsupported,
        Err(error) => error.diagnostic_outcome(),
    }
}

impl<B> ClockedBus for TraceBus<'_, B>
where
    B: ClockedBus + TraceContext,
    B::Error: TraceError,
{
    type Error = B::Error;
    fn instruction_word(&mut self, address: u32, value: u16) {
        self.inner.instruction_word(address, value);
    }
    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        self.inner.instruction_input(address, input);
    }
    fn read_instruction_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, Self::Error> {
        let token = self.begin(Callback::Read, Some(access), start, None, 0);
        let result = self.inner.read_instruction_cycle(access, start);
        let outcome = match &result.result {
            Ok(input) => Outcome::Read(input.value),
            Err(error) => error.diagnostic_outcome(),
        };
        self.end(token, Some(access), result.wait_clocks, outcome);
        result
    }
    fn read_instruction_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: zeff_m68k::timing::InterruptAperture,
    ) -> zeff_m68k::timing::ObservedCycle<InstructionInput, Self::Error> {
        let token = self.begin(Callback::Read, Some(access), start, None, 0);
        let result = self
            .inner
            .read_instruction_cycle_observed(access, start, aperture);
        let outcome = match &result.cycle.result {
            Ok(input) => Outcome::Read(input.value),
            Err(error) => error.diagnostic_outcome(),
        };
        self.end(token, Some(access), result.cycle.wait_clocks, outcome);
        result
    }
    fn read_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: zeff_m68k::timing::InterruptAperture,
    ) -> zeff_m68k::timing::ObservedCycle<u16, Self::Error> {
        let token = self.begin(Callback::Read, Some(access), start, None, 0);
        let result = self.inner.read_cycle_observed(access, start, aperture);
        let outcome = match &result.cycle.result {
            Ok(value) => Outcome::Read(*value),
            Err(error) => error.diagnostic_outcome(),
        };
        self.end(token, Some(access), result.cycle.wait_clocks, outcome);
        result
    }
    fn write_cycle_observed(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> zeff_m68k::timing::ObservedCycle<(), Self::Error> {
        let token = self.begin(Callback::Write, Some(access), start, Some(value), 0);
        let result = self.inner.write_cycle_observed(access, value, start);
        let outcome = match &result.cycle.result {
            Ok(()) => Outcome::Complete,
            Err(error) => error.diagnostic_outcome(),
        };
        self.end(token, Some(access), result.cycle.wait_clocks, outcome);
        result
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
        let token = self.begin(Callback::Write, Some(access), start, Some(value), 0);
        let (result, previous) = self
            .inner
            .write_cycle_observed_with_previous(access, value, start);
        let outcome = match &result.cycle.result {
            Ok(()) => Outcome::Complete,
            Err(error) => error.diagnostic_outcome(),
        };
        self.end(token, Some(access), result.cycle.wait_clocks, outcome);
        (result, previous)
    }

    fn read_discarded_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        let token = self.begin(Callback::Read, Some(access), start, None, 0);
        let result = self.inner.read_discarded_cycle(access, start);
        let outcome = match &result.result {
            Ok(()) => Outcome::Complete,
            Err(error) => error.diagnostic_outcome(),
        };
        self.end(token, Some(access), result.wait_clocks, outcome);
        result
    }

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, Self::Error> {
        let token = self.begin(Callback::Read, Some(access), start, None, 0);
        let result = self.inner.read_cycle(access, start);
        let outcome = match &result.result {
            Ok(value) => Outcome::Read(*value),
            Err(error) => error.diagnostic_outcome(),
        };
        self.end(token, Some(access), result.wait_clocks, outcome);
        result
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        let token = self.begin(Callback::Write, Some(access), start, Some(value), 0);
        let result = self.inner.write_cycle(access, value, start);
        let outcome = match &result.result {
            Ok(()) => Outcome::Complete,
            Err(error) => error.diagnostic_outcome(),
        };
        self.end(token, Some(access), result.wait_clocks, outcome);
        result
    }
    fn write_cycle_with_previous(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> (CycleResult<(), Self::Error>, [Option<u8>; 2]) {
        let token = self.begin(Callback::Write, Some(access), start, Some(value), 0);
        let (result, previous) = self.inner.write_cycle_with_previous(access, value, start);
        let outcome = match &result.result {
            Ok(()) => Outcome::Complete,
            Err(error) => error.diagnostic_outcome(),
        };
        self.end(token, Some(access), result.wait_clocks, outcome);
        (result, previous)
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        let token = self.begin(Callback::Idle, None, start, None, clocks);
        self.inner.idle(start, clocks);
        self.end(token, None, 0, Outcome::Complete);
    }

    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, Self::Error> {
        let token = self.begin(Callback::Acknowledge, Some(access), start, None, 0);
        let result = self.inner.acknowledge_cycle(access, start);
        let outcome = match &result.result {
            Ok(BusResponse::Complete(vector)) => Outcome::Interrupt(*vector),
            Ok(BusResponse::Unsupported) => Outcome::Unsupported,
            Err(error) => error.diagnostic_outcome(),
        };
        self.end(token, Some(access), result.wait_clocks, outcome);
        result
    }

    fn begin_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, Self::Error> {
        let token = self.begin(Callback::BeginRmw, Some(access), start, None, 0);
        let result = self.inner.begin_rmw(access, start);
        self.end(token, Some(access), 0, unit_outcome(&result));
        result
    }

    fn end_rmw(&mut self, end: CpuClocks) {
        let token = self.begin(Callback::EndRmw, None, end, None, 0);
        self.inner.end_rmw(end);
        self.end(token, None, 0, Outcome::Complete);
    }

    fn begin_reset_output(&mut self, start: CpuClocks) -> Result<BusResponse<()>, Self::Error> {
        let token = self.begin(Callback::BeginReset, None, start, None, 0);
        let result = self.inner.begin_reset_output(start);
        self.end(token, None, 0, unit_outcome(&result));
        result
    }

    fn end_reset_output(&mut self, end: CpuClocks) {
        let token = self.begin(Callback::EndReset, None, end, None, 0);
        self.inner.end_reset_output(end);
        self.end(token, None, 0, Outcome::Complete);
    }
}

use super::*;

/// Normal predecessors sample at +1/+2 clocks; handler refill uses +3/+4.
/// Unqualified ownership or timing must return no samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptAperture {
    FollowingPrefetch,
    HandlerRefill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterruptSamples {
    first: u8,
    second: u8,
}

impl InterruptSamples {
    pub fn new(first: u8, second: u8) -> Option<Self> {
        (first < 7 && second < 7).then_some(Self { first, second })
    }

    pub fn levels(self) -> (u8, u8) {
        (self.first, self.second)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedCycle<T, E> {
    pub cycle: CycleResult<T, E>,
    pub samples: Option<InterruptSamples>,
}

impl<T, E> ObservedCycle<T, E> {
    pub fn unobserved(cycle: CycleResult<T, E>) -> Self {
        Self {
            cycle,
            samples: None,
        }
    }
}

impl PrefetchCpu {
    pub(super) fn read_observed_word<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        address: u32,
        function_code: FunctionCode,
        aperture: InterruptAperture,
    ) -> Result<(InstructionInput, Option<InterruptSamples>), TimingFault<B::Error>> {
        if address & 1 != 0 {
            self.frozen = true;
            return Err(TimingFault::AddressError { address });
        }
        let access = BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK,
            function_code,
            kind: AccessKind::Prefetch,
            lanes: ByteLanes::Both,
        };
        self.reserve_transfer()?;
        let observed = bus.read_instruction_cycle_observed(access, self.clock, aperture);
        let samples = (observed.cycle.wait_clocks == 0)
            .then_some(observed.samples)
            .flatten();
        let value = self.finish(observed.cycle, access, Direction::Read)?;
        Ok((value, samples))
    }

    pub(super) fn publish_interrupt_samples(
        &mut self,
        samples: Option<InterruptSamples>,
        final_start: CpuClocks,
        mask: u8,
    ) {
        let samples = (final_start.0.checked_add(4) == Some(self.clock.0))
            .then_some(samples)
            .flatten();
        self.interrupts.observe(samples, mask);
    }

    pub(super) fn immediate_word_absolute<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<(), TimingFault<B::Error>> {
        let pc = pipeline.address;
        bus.instruction_input(pc.wrapping_add(2), pipeline.input(1));
        let high = self.fetch(bus, pc.wrapping_add(4))?;
        bus.instruction_input(pc.wrapping_add(4), high);
        let low = self.fetch(bus, pc.wrapping_add(6))?;
        bus.instruction_input(pc.wrapping_add(6), low);
        let first = self.fetch(bus, pc.wrapping_add(8))?;
        let address = (u32::from(high.value) << 16) | u32::from(low.value);
        let access = self.data_access(address, false)?;
        self.reserve_transfer()?;
        let observed = bus.write_cycle_observed(access, pipeline.words[1], self.clock);
        let samples = (observed.cycle.wait_clocks == 0)
            .then_some(observed.samples)
            .flatten();
        self.finish(observed.cycle, access, Direction::Write)?;
        let final_start = self.clock;
        let second = self.fetch(bus, pc.wrapping_add(10))?;
        crate::alu::logic(
            &mut self.registers,
            crate::ea::Size::Word,
            u32::from(pipeline.words[1]),
        );
        self.registers.pc = pc.wrapping_add(8);
        self.pipeline = Some(Pipeline::sampled(self.registers.pc, [first, second]));
        self.publish_interrupt_samples(
            samples,
            final_start,
            ((self.registers.status_register() >> 8) & 7) as u8,
        );
        Ok(())
    }
}

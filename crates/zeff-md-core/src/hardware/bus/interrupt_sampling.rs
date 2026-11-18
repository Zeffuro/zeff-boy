use super::*;
use crate::hardware::vdp::{VdpInterrupts, VdpRegisterPipeline};
use zeff_m68k::bus::FunctionCode;
use zeff_m68k::timing::{InterruptAperture, InterruptSamples, ObservedCycle};

pub(super) struct NormalInterruptWindow {
    clock: CpuClocks,
    begin: u64,
    samples: InterruptSamples,
}

impl NormalInterruptWindow {
    pub(super) fn begin(bus: &Bus, start: CpuClocks, aperture: InterruptAperture) -> Option<Self> {
        if start != bus.cpu_clock
            || bus.clock_fault
            || bus.rmw.is_some()
            || bus.reset_output
            || bus.irq_level >= 7
            || bus.vdp.interrupts().has_port_history()
        {
            return None;
        }
        let begin = bus.now.get();
        let mut interrupts = *bus.vdp.interrupts();
        let mut registers = *bus.vdp.registers();
        let offsets = match aperture {
            InterruptAperture::FollowingPrefetch => [1, 2],
            InterruptAperture::HandlerRefill => [3, 4],
        };
        let mut levels = [0; 2];
        for (index, offset) in offsets.into_iter().enumerate() {
            let at = MasterTicks::new(begin.checked_add(offset * MAIN_CPU_DIVISOR)?);
            levels[index] = if bus.irq_level != 0 {
                bus.irq_level
            } else {
                project(&mut interrupts, &mut registers, at, bus.region)?
            };
        }
        Some(Self {
            clock: start,
            begin,
            samples: InterruptSamples::new(levels[0], levels[1])?,
        })
    }

    pub(super) fn finish<T, E>(self, bus: &Bus, cycle: CycleResult<T, E>) -> ObservedCycle<T, E> {
        let samples = (cycle.result.is_ok()
            && cycle.wait_clocks == 0
            && !bus.clock_fault
            && self.clock.0.checked_add(u64::from(TRANSFER_CLOCKS)) == Some(bus.cpu_clock.0)
            && self
                .begin
                .checked_add(u64::from(TRANSFER_CLOCKS) * MAIN_CPU_DIVISOR)
                == Some(bus.now.get()))
        .then_some(self.samples);
        ObservedCycle { cycle, samples }
    }
}

fn project(
    interrupts: &mut VdpInterrupts,
    registers: &mut VdpRegisterPipeline,
    at: MasterTicks,
    region: Region,
) -> Option<u8> {
    while interrupts
        .advance_pair(at, region, registers, false)
        .ok()?
        .is_some()
    {}
    interrupts.interrupt_level(registers).ok()
}

impl Bus {
    fn inert_prefetch(access: BusAccess) -> bool {
        access.kind == AccessKind::Prefetch
            && access.lanes == ByteLanes::Both
            && matches!(
                access.function_code,
                FunctionCode::SupervisorProgram | FunctionCode::UserProgram
            )
            && (access.bus_address <= CARTRIDGE_END || access.bus_address >= WORK_RAM_START)
    }

    pub(super) fn read_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: InterruptAperture,
    ) -> ObservedCycle<u16, BusError> {
        let window = Self::inert_prefetch(access)
            .then(|| NormalInterruptWindow::begin(self, start, aperture))
            .flatten();
        let cycle = self.read_cycle(access, start);
        match window {
            Some(window) => window.finish(self, cycle),
            None => ObservedCycle::unobserved(cycle),
        }
    }

    pub(super) fn read_input_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: InterruptAperture,
    ) -> ObservedCycle<InstructionInput, BusError> {
        let window = Self::inert_prefetch(access)
            .then(|| NormalInterruptWindow::begin(self, start, aperture))
            .flatten();
        let cycle = self.read_input_cycle_timed(access, start);
        match window {
            Some(window) => window.finish(self, cycle),
            None => ObservedCycle::unobserved(cycle),
        }
    }

    pub(super) fn write_observed(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> ObservedCycle<(), BusError> {
        self.write_observed_capturing(access, value, start, &mut ())
    }

    pub(super) fn write_observed_capturing<S: previous::Capture>(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
        previous: &mut S,
    ) -> ObservedCycle<(), BusError> {
        let window = (access.kind == AccessKind::Data && access.lanes == ByteLanes::Both)
            .then(|| {
                NormalInterruptWindow::begin(self, start, InterruptAperture::FollowingPrefetch)
            })
            .flatten();
        let cycle = self.write_cycle_capturing(access, value, start, previous);
        match window {
            Some(window) => window.finish(self, cycle),
            None => ObservedCycle::unobserved(cycle),
        }
    }
}

impl<C: crate::sound_trace::SoundCapture> scheduled::ScheduledBus<'_, C> {
    pub(super) fn read_observed_with<E>(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: InterruptAperture,
        error: impl FnOnce(BusError) -> E,
    ) -> ObservedCycle<u16, E> {
        let window = Bus::inert_prefetch(access)
            .then(|| NormalInterruptWindow::begin(self.bus, start, aperture))
            .flatten();
        let cycle = self.read_cycle_with(access, start, error);
        match window {
            Some(window) => window.finish(self.bus, cycle),
            None => ObservedCycle::unobserved(cycle),
        }
    }

    pub(super) fn read_input_observed_with<E>(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: InterruptAperture,
        error: impl FnOnce(BusError) -> E,
    ) -> ObservedCycle<InstructionInput, E> {
        let window = Bus::inert_prefetch(access)
            .then(|| NormalInterruptWindow::begin(self.bus, start, aperture))
            .flatten();
        let cycle = self.read_input_cycle_with(access, start, error);
        match window {
            Some(window) => window.finish(self.bus, cycle),
            None => ObservedCycle::unobserved(cycle),
        }
    }

    pub(super) fn write_observed_with<E>(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
        error: impl FnOnce(BusError) -> E,
    ) -> ObservedCycle<(), E> {
        self.write_observed_with_capturing(access, value, start, error, &mut ())
    }

    pub(super) fn write_observed_with_capturing<E, S: previous::Capture>(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
        error: impl FnOnce(BusError) -> E,
        previous: &mut S,
    ) -> ObservedCycle<(), E> {
        let window = (access.kind == AccessKind::Data && access.lanes == ByteLanes::Both)
            .then(|| {
                NormalInterruptWindow::begin(self.bus, start, InterruptAperture::FollowingPrefetch)
            })
            .flatten();
        let cycle = self.write_cycle_capturing(access, value, start, error, previous);
        match window {
            Some(window) => window.finish(self.bus, cycle),
            None => ObservedCycle::unobserved(cycle),
        }
    }
}

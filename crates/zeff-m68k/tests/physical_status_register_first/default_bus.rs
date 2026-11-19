use super::*;
use device::Device;

pub(super) trait FixtureBus:
    InstructionBus<Error = &'static str>
    + ClockedBus<Error = &'static str>
    + Clone
    + PartialEq
    + std::fmt::Debug
{
    fn device(&self) -> &Device;
}

impl FixtureBus for Device {
    fn device(&self) -> &Device {
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DefaultBus(pub Device);

impl FixtureBus for DefaultBus {
    fn device(&self) -> &Device {
        &self.0
    }
}

impl EntryBus for DefaultBus {
    type Error = &'static str;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.0.read_entry(access, start)
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.0.write_entry(access, value, start)
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.0.idle_entry(start, clocks);
    }
}

impl InstructionBus for DefaultBus {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.0.read_instruction(access, start)
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.0.write_instruction(access, value, start)
    }
}

impl ClockedBus for DefaultBus {
    type Error = &'static str;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, Self::Error> {
        self.0.read_cycle(access, start)
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        self.0.write_cycle(access, value, start)
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        ClockedBus::idle(&mut self.0, start, clocks);
    }
}

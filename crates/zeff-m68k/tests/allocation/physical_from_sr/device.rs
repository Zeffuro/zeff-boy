use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u16, u64, u32),
    Write(BusAccess, u16, u64, u32),
    Idle(u64, u32),
}

pub(super) struct Device<'a> {
    pub(super) memory: &'a mut MemoryBus,
    pub(super) events: [Option<Event>; 8],
    pub(super) event_count: usize,
    pub(super) transfers: usize,
    pub(super) clock: u64,
    pub(super) host: Option<usize>,
    pub(super) maximum: Option<usize>,
}

impl Device<'_> {
    fn event(&mut self, event: Event, start: CpuClocks, elapsed: u32) {
        assert_eq!(self.clock, start.0);
        self.events[self.event_count] = Some(event);
        self.event_count += 1;
        self.clock += u64::from(elapsed);
    }
    fn elapsed(&self) -> u32 {
        if self.maximum == Some(self.transfers) {
            u32::MAX
        } else {
            7
        }
    }
}

impl EntryBus for Device<'_> {
    type Error = ();
    fn read_entry(&mut self, _: BusAccess, _: CpuClocks) -> EntryReport<u16, ()> {
        panic!("unexpected entry read")
    }
    fn write_entry(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> EntryReport<(), ()> {
        panic!("unexpected entry write")
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.event(Event::Idle(start.0, clocks), start, clocks);
    }
}

impl InstructionBus for Device<'_> {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, ()> {
        assert_eq!(access.logical_address & 1, 0);
        let value = self.memory.read(access).unwrap();
        let elapsed = self.elapsed();
        let failed = self.host == Some(self.transfers);
        self.event(Event::Read(access, value, start.0, elapsed), start, elapsed);
        self.transfers += 1;
        InstructionReport {
            elapsed,
            outcome: if failed {
                InstructionOutcome::HostFailure(())
            } else {
                InstructionOutcome::Complete(value)
            },
        }
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), ()> {
        assert_eq!(access.logical_address & 1, 0);
        let elapsed = self.elapsed();
        let failed = self.host == Some(self.transfers);
        self.memory.write(access, value).unwrap();
        self.event(
            Event::Write(access, value, start.0, elapsed),
            start,
            elapsed,
        );
        self.transfers += 1;
        InstructionReport {
            elapsed,
            outcome: if failed {
                InstructionOutcome::HostFailure(())
            } else {
                InstructionOutcome::Complete(())
            },
        }
    }
}

pub(super) struct Snapshot {
    memory: [u8; 0x8000],
    pointer: *const MemoryBus,
    events: [Option<Event>; 8],
    event_count: usize,
    transfers: usize,
    clock: u64,
    host: Option<usize>,
    maximum: Option<usize>,
}

impl Device<'_> {
    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            memory: self.memory.0,
            pointer: self.memory,
            events: self.events,
            event_count: self.event_count,
            transfers: self.transfers,
            clock: self.clock,
            host: self.host,
            maximum: self.maximum,
        }
    }
}

impl Snapshot {
    pub(super) fn matches(&self, device: &Device<'_>) -> bool {
        self.memory == device.memory.0
            && std::ptr::eq(self.pointer, device.memory)
            && self.events == device.events
            && self.event_count == device.event_count
            && self.transfers == device.transfers
            && self.clock == device.clock
            && self.host == device.host
            && self.maximum == device.maximum
    }
}

impl ClockedBus for Device<'_> {
    type Error = ();
    fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, ()> {
        panic!("unexpected ordinary read")
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), ()> {
        panic!("unexpected ordinary write")
    }
    fn idle(&mut self, _: CpuClocks, _: u32) {
        panic!("unexpected ordinary idle")
    }
}

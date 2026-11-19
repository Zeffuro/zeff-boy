use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Ram(pub(super) [Option<(u32, u16)>; 32]);

impl Ram {
    pub(super) fn read(&self, address: u32) -> u16 {
        self.0
            .iter()
            .flatten()
            .find(|(at, _)| *at == address & 0x00ff_fffe)
            .map_or(0, |(_, value)| *value)
    }

    pub(super) fn write(&mut self, address: u32, value: u16) {
        let address = address & 0x00ff_fffe;
        if let Some(slot) = self.0.iter_mut().flatten().find(|(at, _)| *at == address) {
            slot.1 = value;
        } else {
            *self.0.iter_mut().find(|slot| slot.is_none()).unwrap() = Some((address, value));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u16, u64, u32, Report),
    Write(BusAccess, u16, u64, u32),
    Idle(u64, u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub(super) ram: Ram,
    pub(super) events: [Option<Event>; 24],
    pub(super) count: usize,
    pub(super) instruction_transfers: usize,
    pub(super) entry_transfers: usize,
    pub(super) clock: u64,
    pub(super) failure: Option<(usize, Report)>,
    pub(super) maximum: Option<usize>,
}

impl Device {
    pub(super) fn new(ram: Ram, clock: u64, path: Path, immediate: bool) -> Self {
        Self {
            ram,
            events: [None; 24],
            count: 0,
            instruction_transfers: 0,
            entry_transfers: 0,
            clock,
            failure: if let Path::Report(at, report) = path {
                Some((at, report))
            } else {
                None
            },
            maximum: if immediate && matches!(path, Path::Idle(_, _)) {
                Some(0)
            } else {
                None
            },
        }
    }

    fn event(&mut self, event: Event, start: CpuClocks, elapsed: u32) {
        assert_eq!(self.clock, start.0);
        self.events[self.count] = Some(event);
        self.count += 1;
        self.clock += u64::from(elapsed);
    }
}

impl EntryBus for Device {
    type Error = usize;

    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, usize> {
        assert_eq!(access.logical_address & 1, 0);
        let value = self.ram.read(access.bus_address);
        self.event(
            Event::Read(access, value, start.0, 4, Report::Complete),
            start,
            4,
        );
        self.entry_transfers += 1;
        EntryReport {
            elapsed: 4,
            outcome: EntryOutcome::Complete(value),
        }
    }

    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), usize> {
        assert_eq!(access.logical_address & 1, 0);
        self.ram.write(access.bus_address, value);
        self.event(Event::Write(access, value, start.0, 4), start, 4);
        self.entry_transfers += 1;
        EntryReport {
            elapsed: 4,
            outcome: EntryOutcome::Complete(()),
        }
    }

    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.event(Event::Idle(start.0, clocks), start, clocks);
    }
}

impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, usize> {
        assert_eq!(access.logical_address & 1, 0);
        let at = self.instruction_transfers;
        let report = self
            .failure
            .filter(|(target, _)| *target == at)
            .map_or(Report::Complete, |(_, kind)| kind);
        let elapsed = if self.maximum == Some(at) {
            u32::MAX
        } else {
            report.elapsed()
        };
        let value = self.ram.read(access.bus_address);
        self.event(
            Event::Read(access, value, start.0, elapsed, report),
            start,
            elapsed,
        );
        self.instruction_transfers += 1;
        InstructionReport {
            elapsed,
            outcome: match report {
                Report::Host => InstructionOutcome::HostFailure(at),
                Report::Boundary => InstructionOutcome::AtGroup0Entry,
                Report::Unavailable => InstructionOutcome::TimingUnavailable,
                Report::Complete | Report::Short => InstructionOutcome::Complete(value),
            },
        }
    }

    fn write_instruction(
        &mut self,
        _: BusAccess,
        _: u16,
        _: CpuClocks,
    ) -> InstructionReport<(), usize> {
        panic!("unexpected status instruction write")
    }
}

impl ClockedBus for Device {
    type Error = usize;

    fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, usize> {
        panic!("unexpected ordinary read")
    }

    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), usize> {
        panic!("unexpected ordinary write")
    }

    fn idle(&mut self, _: CpuClocks, _: u32) {
        panic!("unexpected ordinary idle")
    }
}

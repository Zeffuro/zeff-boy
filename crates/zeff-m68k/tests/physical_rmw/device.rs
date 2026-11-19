use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Complete,
    Host,
    Boundary,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Instruction,
    Entry,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Transfer {
        stage: Stage,
        access: BusAccess,
        direction: Direction,
        value: u16,
        start: u64,
        elapsed: u32,
        outcome: Kind,
        deposited: bool,
    },
    Idle(u64, u32),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub instruction_count: usize,
    pub entry_count: usize,
    pub injection: Option<(usize, Kind, u32, bool)>,
}
impl Device {
    pub fn new(clock: u64) -> Self {
        Self {
            clock,
            words: BTreeMap::new(),
            events: Vec::new(),
            instruction_count: 0,
            entry_count: 0,
            injection: None,
        }
    }
    fn sample(&self, address: u32) -> u16 {
        self.words.get(&(address & 0x00ff_fffe)).copied().unwrap()
    }
    fn deposit(&mut self, access: BusAccess, value: u16) {
        assert_eq!(access.lanes, ByteLanes::Both);
        self.words.insert(access.bus_address, value);
    }
    fn report(
        &mut self,
        access: BusAccess,
        direction: Direction,
        value: u16,
        start: CpuClocks,
    ) -> (Kind, u32) {
        assert_eq!(self.clock, start.0);
        let (_, outcome, elapsed, failed_deposit) = self
            .injection
            .filter(|(at, _, _, _)| *at == self.instruction_count)
            .unwrap_or((self.instruction_count, Kind::Complete, 4, false));
        let deposited =
            direction == Direction::Write && (outcome == Kind::Complete || failed_deposit);
        if deposited {
            self.deposit(access, value);
        }
        self.events.push(Event::Transfer {
            stage: Stage::Instruction,
            access,
            direction,
            value,
            start: start.0,
            elapsed,
            outcome,
            deposited,
        });
        self.instruction_count += 1;
        self.clock += u64::from(elapsed);
        (outcome, elapsed)
    }
    fn entry(&mut self, access: BusAccess, direction: Direction, value: u16, start: CpuClocks) {
        assert_eq!(self.clock, start.0);
        if direction == Direction::Write {
            self.deposit(access, value);
        }
        self.events.push(Event::Transfer {
            stage: Stage::Entry,
            access,
            direction,
            value,
            start: start.0,
            elapsed: 4,
            outcome: Kind::Complete,
            deposited: direction == Direction::Write,
        });
        self.entry_count += 1;
        self.clock += 4;
    }
}
impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        let value = self.sample(access.logical_address);
        let (outcome, elapsed) = self.report(access, Direction::Read, value, start);
        InstructionReport {
            elapsed,
            outcome: match outcome {
                Kind::Complete => InstructionOutcome::Complete(value),
                Kind::Host => InstructionOutcome::HostFailure("host"),
                Kind::Boundary => InstructionOutcome::AtGroup0Entry,
                Kind::Unavailable => InstructionOutcome::TimingUnavailable,
            },
        }
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        let (outcome, elapsed) = self.report(access, Direction::Write, value, start);
        InstructionReport {
            elapsed,
            outcome: match outcome {
                Kind::Complete => InstructionOutcome::Complete(()),
                Kind::Host => InstructionOutcome::HostFailure("host"),
                Kind::Boundary => InstructionOutcome::AtGroup0Entry,
                Kind::Unavailable => InstructionOutcome::TimingUnavailable,
            },
        }
    }
}
impl EntryBus for Device {
    type Error = &'static str;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, Self::Error> {
        let value = self.sample(access.logical_address);
        self.entry(access, Direction::Read, value, start);
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
    ) -> EntryReport<(), Self::Error> {
        self.entry(access, Direction::Write, value, start);
        EntryReport {
            elapsed: 4,
            outcome: EntryOutcome::Complete(()),
        }
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(self.clock, start.0);
        self.events.push(Event::Idle(start.0, clocks));
        self.clock += u64::from(clocks);
    }
}
impl ClockedBus for Device {
    type Error = &'static str;
    fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, Self::Error> {
        panic!("unexpected ordinary read")
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), Self::Error> {
        panic!("unexpected ordinary write")
    }
    fn idle(&mut self, _: CpuClocks, _: u32) {
        panic!("unexpected ordinary idle")
    }
}

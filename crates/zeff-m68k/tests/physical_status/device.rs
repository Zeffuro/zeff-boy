use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Complete,
    Host,
    Boundary,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u64, u16),
    Detect(BusAccess, u64),
    EntryRead(BusAccess, u64, u16),
    EntryWrite(BusAccess, u64, u16),
    EntryDetect(BusAccess, Direction, u64),
    Idle(u64, u32),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub elapsed: [u32; 5],
    pub failure: Option<(usize, bool)>,
    pub report: Option<(usize, Kind, u32)>,
    pub detect_elapsed: u32,
    pub detector: Option<(Kind, u32)>,
    pub default_detector: bool,
    pub default_entry_detector: bool,
    pub count: usize,
    pub entry_count: usize,
    pub entry_failure: Option<(usize, Kind, u32)>,
    pub entry_detector: Option<(Kind, u32)>,
    pub mutation: Option<(usize, u32, u16)>,
}
impl Device {
    pub fn new(clock: u64) -> Self {
        let mut bus = Self {
            clock,
            words: BTreeMap::new(),
            events: Vec::new(),
            elapsed: [4; 5],
            failure: None,
            report: None,
            detect_elapsed: 4,
            detector: None,
            default_detector: false,
            default_entry_detector: false,
            count: 0,
            entry_count: 0,
            entry_failure: None,
            entry_detector: None,
            mutation: None,
        };
        for (a, v) in [
            (8, 0),
            (10, 0x6000),
            (12, 0),
            (14, 0x6000),
            (0x6000, 0x4e71),
            (0x6002, 0x7201),
        ] {
            bus.word(a, v);
        }
        bus
    }
    pub fn word(&mut self, address: u32, value: u16) {
        self.words.insert(address & 0xfffffe, value);
    }
    pub fn sample(&self, address: u32) -> u16 {
        self.words
            .get(&(address & 0xfffffe))
            .copied()
            .unwrap_or(0x4e71)
    }
    fn start(&self, clock: CpuClocks) {
        assert_eq!(self.clock, clock.0);
    }
    fn instruction<T>(
        &mut self,
        kind: Kind,
        elapsed: u32,
        value: T,
    ) -> InstructionReport<T, &'static str> {
        self.clock += u64::from(elapsed);
        InstructionReport {
            elapsed,
            outcome: match kind {
                Kind::Complete => InstructionOutcome::Complete(value),
                Kind::Host => InstructionOutcome::HostFailure("host"),
                Kind::Boundary => InstructionOutcome::AtGroup0Entry,
                Kind::Unavailable => InstructionOutcome::TimingUnavailable,
            },
        }
    }
    fn entry<T>(&mut self, clock: CpuClocks, value: T) -> EntryReport<T, &'static str> {
        self.start(clock);
        let at = self.entry_count;
        self.entry_count += 1;
        let (_, kind, elapsed) =
            self.entry_failure
                .filter(|(n, _, _)| *n == at)
                .unwrap_or((at, Kind::Complete, 4));
        self.clock += u64::from(elapsed);
        EntryReport {
            elapsed,
            outcome: match kind {
                Kind::Complete => EntryOutcome::Complete(value),
                Kind::Host => EntryOutcome::HostFailure("host"),
                Kind::Boundary => EntryOutcome::CriticalAbortReleased,
                Kind::Unavailable => EntryOutcome::TimingUnavailable,
            },
        }
    }
}
impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        clock: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.start(clock);
        let at = self.count;
        if let Some((index, address, value)) = self.mutation
            && index == at
        {
            self.word(address, value);
            self.mutation = None;
        }
        let value = self.sample(access.logical_address);
        self.events.push(Event::Read(access, clock.0, value));
        self.count += 1;
        let (kind, elapsed) =
            if let Some((_, kind, elapsed)) = self.report.filter(|(n, _, _)| *n == at) {
                (kind, elapsed)
            } else {
                let kind = match self.failure.filter(|(n, _)| *n == at) {
                    Some((_, true)) => Kind::Boundary,
                    Some((_, false)) => Kind::Host,
                    None => Kind::Complete,
                };
                (kind, self.elapsed[at])
            };
        self.instruction(kind, elapsed, value)
    }
    fn write_instruction(
        &mut self,
        _: BusAccess,
        _: u16,
        _: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        panic!("status write");
    }
    fn detect_instruction_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        clock: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.start(clock);
        assert_eq!(direction, Direction::Read);
        self.events.push(Event::Detect(access, clock.0));
        let (kind, elapsed) = if self.default_detector {
            (Kind::Unavailable, 0)
        } else {
            self.detector
                .unwrap_or((Kind::Boundary, self.detect_elapsed))
        };
        self.instruction(kind, elapsed, ())
    }
}
impl EntryBus for Device {
    type Error = &'static str;
    fn read_entry(&mut self, access: BusAccess, clock: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.start(clock);
        let value = self.sample(access.logical_address);
        self.events.push(Event::EntryRead(access, clock.0, value));
        self.entry(clock, value)
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.start(clock);
        self.events.push(Event::EntryWrite(access, clock.0, value));
        self.word(access.logical_address, value);
        self.entry(clock, ())
    }
    fn idle_entry(&mut self, clock: CpuClocks, clocks: u32) {
        self.start(clock);
        self.events.push(Event::Idle(clock.0, clocks));
        self.clock += u64::from(clocks);
    }
    fn detect_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        clock: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.start(clock);
        self.events
            .push(Event::EntryDetect(access, direction, clock.0));
        let (kind, elapsed) = self.entry_detector.unwrap_or((Kind::Unavailable, 0));
        self.clock += u64::from(elapsed);
        EntryReport {
            elapsed,
            outcome: match kind {
                Kind::Complete => EntryOutcome::Complete(()),
                Kind::Host => EntryOutcome::HostFailure("host"),
                Kind::Boundary => EntryOutcome::CriticalAbortReleased,
                Kind::Unavailable => EntryOutcome::TimingUnavailable,
            },
        }
    }
}
impl ClockedBus for Device {
    type Error = &'static str;
    fn read_cycle(&mut self, access: BusAccess, clock: CpuClocks) -> CycleResult<u16, Self::Error> {
        self.start(clock);
        let value = self.sample(access.logical_address);
        self.events.push(Event::Read(access, clock.0, value));
        self.clock += 4;
        CycleResult::ready(value)
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        self.start(clock);
        self.word(access.logical_address, value);
        self.events.push(Event::EntryWrite(access, clock.0, value));
        self.clock += 4;
        CycleResult::ready(())
    }
    fn idle(&mut self, clock: CpuClocks, clocks: u32) {
        self.idle_entry(clock, clocks);
    }
}

pub(super) struct DefaultDetector<'a>(pub &'a mut Device);
impl EntryBus for DefaultDetector<'_> {
    type Error = &'static str;
    fn read_entry(&mut self, a: BusAccess, c: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.0.read_entry(a, c)
    }
    fn write_entry(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> EntryReport<(), Self::Error> {
        self.0.write_entry(a, v, c)
    }
    fn idle_entry(&mut self, c: CpuClocks, n: u32) {
        self.0.idle_entry(c, n);
    }
    fn detect_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.0.detect_address_error(a, d, c)
    }
}
impl InstructionBus for DefaultDetector<'_> {
    fn read_instruction(
        &mut self,
        a: BusAccess,
        c: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.0.read_instruction(a, c)
    }
    fn write_instruction(
        &mut self,
        a: BusAccess,
        v: u16,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.0.write_instruction(a, v, c)
    }
}

pub(super) struct DefaultEntryDetector<'a>(pub &'a mut Device);
impl EntryBus for DefaultEntryDetector<'_> {
    type Error = &'static str;
    fn read_entry(&mut self, a: BusAccess, c: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.0.read_entry(a, c)
    }
    fn write_entry(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> EntryReport<(), Self::Error> {
        self.0.write_entry(a, v, c)
    }
    fn idle_entry(&mut self, c: CpuClocks, n: u32) {
        self.0.idle_entry(c, n);
    }
}
impl InstructionBus for DefaultEntryDetector<'_> {
    fn read_instruction(
        &mut self,
        a: BusAccess,
        c: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.0.read_instruction(a, c)
    }
    fn write_instruction(
        &mut self,
        a: BusAccess,
        v: u16,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.0.write_instruction(a, v, c)
    }
    fn detect_instruction_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.0.detect_instruction_address_error(a, d, c)
    }
}

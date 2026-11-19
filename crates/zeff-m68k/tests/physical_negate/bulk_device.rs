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
    Transfer(Stage, BusAccess, Direction, u16, u64, u32, Kind, bool),
    Detect(Stage, BusAccess, Direction, u64, u32, Kind),
    Idle(u64, u32),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub elapsed: [u32; 8],
    pub report: Option<(usize, Kind, u32)>,
    pub write_deposit: bool,
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
        let mut b = Self {
            clock,
            words: BTreeMap::new(),
            events: Vec::new(),
            elapsed: [4; 8],
            report: None,
            write_deposit: true,
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
            (10, 0x4000),
            (12, 0),
            (14, 0x4000),
            (0x4000, 0x4e71),
            (0x4002, 0x7002),
        ] {
            b.word(a, v);
        }
        b
    }
    pub fn word(&mut self, a: u32, v: u16) {
        self.words.insert(a & 0x00ff_fffe, v);
    }
    pub fn sample(&self, a: u32) -> u16 {
        self.words
            .get(&(a & 0x00ff_fffe))
            .copied()
            .unwrap_or(0x4e71)
    }
    fn instruction(&mut self, a: BusAccess, d: Direction, v: u16, c: CpuClocks) -> (Kind, u32) {
        assert_eq!(self.clock, c.0);
        if let Some((n, at, value)) = self.mutation
            && n == self.count
        {
            self.word(at, value);
        }
        let v = if d == Direction::Read {
            self.sample(a.logical_address)
        } else {
            v
        };
        let (k, n) = bulk_model::report(self, self.count);
        let deposited =
            d == Direction::Write && (k == Kind::Complete && n >= 4 || self.write_deposit);
        if deposited {
            let old = self.sample(a.logical_address);
            let word = match a.lanes {
                ByteLanes::Both => v,
                ByteLanes::Upper => old & 0x00ff | v & 0xff00,
                ByteLanes::Lower => old & 0xff00 | v & 0x00ff,
            };
            self.word(a.logical_address, word);
        }
        self.events.push(Event::Transfer(
            Stage::Instruction,
            a,
            d,
            v,
            c.0,
            n,
            k,
            deposited,
        ));
        self.clock += u64::from(n);
        self.count += 1;
        (k, n)
    }
    fn entry(&mut self, a: BusAccess, d: Direction, v: u16, c: CpuClocks) -> (Kind, u32) {
        assert_eq!(self.clock, c.0);
        let (_, k, n) = self
            .entry_failure
            .filter(|(at, _, _)| *at == self.entry_count)
            .unwrap_or((self.entry_count, Kind::Complete, 4));
        if d == Direction::Write {
            self.word(a.logical_address, v);
        }
        self.events.push(Event::Transfer(
            Stage::Entry,
            a,
            d,
            v,
            c.0,
            n,
            k,
            d == Direction::Write,
        ));
        self.clock += u64::from(n);
        self.entry_count += 1;
        (k, n)
    }
}
fn instruction_outcome<T>(k: Kind, v: T) -> InstructionOutcome<T, &'static str> {
    match k {
        Kind::Complete => InstructionOutcome::Complete(v),
        Kind::Host => InstructionOutcome::HostFailure("host"),
        Kind::Boundary => InstructionOutcome::AtGroup0Entry,
        Kind::Unavailable => InstructionOutcome::TimingUnavailable,
    }
}
fn entry_outcome<T>(k: Kind, v: T) -> EntryOutcome<T, &'static str> {
    match k {
        Kind::Complete => EntryOutcome::Complete(v),
        Kind::Host => EntryOutcome::HostFailure("host"),
        Kind::Boundary => EntryOutcome::CriticalAbortReleased,
        Kind::Unavailable => EntryOutcome::TimingUnavailable,
    }
}
impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        a: BusAccess,
        c: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        let (k, n) = self.instruction(a, Direction::Read, 0, c);
        let value = self.sample(a.logical_address);
        InstructionReport {
            elapsed: n,
            outcome: instruction_outcome(k, value),
        }
    }
    fn write_instruction(
        &mut self,
        a: BusAccess,
        v: u16,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        let (k, n) = self.instruction(a, Direction::Write, v, c);
        InstructionReport {
            elapsed: n,
            outcome: instruction_outcome(k, ()),
        }
    }
    fn detect_instruction_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        assert_eq!(self.clock, c.0);
        let (k, n) = self.detector.unwrap_or((Kind::Boundary, 8));
        self.events
            .push(Event::Detect(Stage::Instruction, a, d, c.0, n, k));
        self.clock += u64::from(n);
        InstructionReport {
            elapsed: n,
            outcome: instruction_outcome(k, ()),
        }
    }
}
impl EntryBus for Device {
    type Error = &'static str;
    fn read_entry(&mut self, a: BusAccess, c: CpuClocks) -> EntryReport<u16, Self::Error> {
        let v = self.sample(a.logical_address);
        let (k, n) = self.entry(a, Direction::Read, v, c);
        EntryReport {
            elapsed: n,
            outcome: entry_outcome(k, v),
        }
    }
    fn write_entry(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> EntryReport<(), Self::Error> {
        let (k, n) = self.entry(a, Direction::Write, v, c);
        EntryReport {
            elapsed: n,
            outcome: entry_outcome(k, ()),
        }
    }
    fn idle_entry(&mut self, c: CpuClocks, n: u32) {
        assert_eq!(self.clock, c.0);
        self.events.push(Event::Idle(c.0, n));
        self.clock += u64::from(n);
    }
    fn detect_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        assert_eq!(self.clock, c.0);
        let (k, n) = self.entry_detector.unwrap_or((Kind::Unavailable, 0));
        self.events
            .push(Event::Detect(Stage::Entry, a, d, c.0, n, k));
        self.clock += u64::from(n);
        EntryReport {
            elapsed: n,
            outcome: entry_outcome(k, ()),
        }
    }
}
impl ClockedBus for Device {
    type Error = &'static str;
    fn read_cycle(&mut self, a: BusAccess, c: CpuClocks) -> CycleResult<u16, Self::Error> {
        assert_eq!(self.clock, c.0);
        let v = self.sample(a.logical_address);
        self.events.push(Event::Transfer(
            Stage::Instruction,
            a,
            Direction::Read,
            v,
            c.0,
            4,
            Kind::Complete,
            false,
        ));
        self.clock += 4;
        CycleResult::ready(v)
    }
    fn write_cycle(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> CycleResult<(), Self::Error> {
        assert_eq!(self.clock, c.0);
        self.word(a.logical_address, v);
        self.events.push(Event::Transfer(
            Stage::Instruction,
            a,
            Direction::Write,
            v,
            c.0,
            4,
            Kind::Complete,
            true,
        ));
        self.clock += 4;
        CycleResult::ready(())
    }
    fn idle(&mut self, c: CpuClocks, n: u32) {
        self.idle_entry(c, n);
    }
}
pub(super) struct DefaultPrimary<'a>(pub &'a mut Device);
pub(super) struct DefaultSecondary<'a>(pub &'a mut Device);
pub(super) struct DefaultBoth<'a>(pub &'a mut Device);
macro_rules! entry_forward {
    ($name:ident, $($detector:item)*) => {
        impl EntryBus for $name<'_> {
            type Error = &'static str;
            fn read_entry(&mut self, a: BusAccess, c: CpuClocks) -> EntryReport<u16, Self::Error> {
                self.0.read_entry(a, c)
            }
            fn write_entry(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> EntryReport<(), Self::Error> {
                self.0.write_entry(a, v, c)
            }
            fn idle_entry(&mut self, c: CpuClocks, n: u32) {
                self.0.idle_entry(c, n)
            }
            $($detector)*
        }
    };
}
entry_forward!(
    DefaultPrimary,
    fn detect_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.0.detect_address_error(a, d, c)
    }
);
entry_forward!(DefaultSecondary,);
entry_forward!(DefaultBoth,);
macro_rules! instruction_forward {
    ($name:ident, $($detector:item)*) => {
        impl InstructionBus for $name<'_> {
            fn read_instruction(&mut self, a: BusAccess, c: CpuClocks) -> InstructionReport<u16, Self::Error> {
                self.0.read_instruction(a, c)
            }
            fn write_instruction(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> InstructionReport<(), Self::Error> {
                self.0.write_instruction(a, v, c)
            }
            $($detector)*
        }
    };
}
instruction_forward!(DefaultPrimary,);
instruction_forward!(
    DefaultSecondary,
    fn detect_instruction_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.0.detect_instruction_address_error(a, d, c)
    }
);
instruction_forward!(DefaultBoth,);

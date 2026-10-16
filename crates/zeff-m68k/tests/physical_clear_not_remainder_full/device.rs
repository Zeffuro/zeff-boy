use super::fixed::{Event, Events, Ram, Scripts};
use super::model::{Case, Failure, Report};
use super::{
    AccessKind, BusAccess, ByteLanes, ClockedBus, CpuClocks, CycleResult, Direction, EntryBus,
    EntryOutcome, EntryReport, FunctionCode, InstructionBus, InstructionOutcome, InstructionReport,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Mutation {
    pub at: usize,
    pub address: u32,
    pub word: u16,
    pub after: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub words: Ram,
    pub events: Events,
    pub scripts: Scripts,
    pub clock: u64,
    pub count: usize,
    pub entry_count: usize,
    pub primary: [Report; 6],
    pub primary_deposit: [bool; 6],
    pub entry: [Report; 16],
    pub entry_deposit: [bool; 16],
    pub secondary: Report,
    pub default_secondary: bool,
    pub mutations: [Option<Mutation>; 2],
    pub mutated: usize,
    pub counts: [usize; 5],
    pub ordinary: [usize; 2],
    pub before_entry: Option<(u64, Ram)>,
    pub allow_legacy: bool,
    pub profile: super::model::Profile,
    pub phases: [Option<super::InstructionPhase>; 6],
}
impl Device {
    pub fn seed(c: Case) -> Self {
        let mut b = Self {
            words: Ram::default(),
            events: Events::default(),
            scripts: Scripts::default(),
            clock: c.start,
            count: 0,
            entry_count: 0,
            primary: [Report::default(); 6],
            primary_deposit: [false; 6],
            entry: [Report::default(); 16],
            entry_deposit: [false; 16],
            secondary: Report {
                kind: Failure::Boundary,
                elapsed: 7,
            },
            default_secondary: true,
            mutations: [None; 2],
            mutated: 0,
            counts: [0; 5],
            ordinary: [0; 2],
            before_entry: None,
            allow_legacy: false,
            profile: c.p,
            phases: [None; 6],
        };
        b.words.insert(c.r.pc, c.word);
        b.words.insert(c.r.pc.wrapping_add(2), c.retained);
        if c.p.extensions() == 2 {
            b.words.insert(c.r.pc.wrapping_add(4), c.low);
        }
        b.words.insert(c.r.pc.wrapping_add(c.p.length()), c.next);
        b.words
            .insert(c.r.pc.wrapping_add(c.p.length() + 2), c.last);
        for (address, value) in [
            (0, (c.reset_ssp >> 16) as u16),
            (2, c.reset_ssp as u16),
            (4, (c.reset_handler >> 16) as u16),
            (6, c.reset_handler as u16),
            (8, (c.handler >> 16) as u16),
            (10, c.handler as u16),
            (c.handler, 0x4e71),
            (c.handler.wrapping_add(2), 0x7002),
            (c.reset_handler, 0x4e71),
            (c.reset_handler.wrapping_add(2), 0x7002),
        ] {
            b.words.insert(address, value);
        }
        if c.p.memory() {
            b.words.insert(super::model::address(c, &c.r), c.operand);
        }
        b
    }
    pub fn fault(&mut self, at: usize, kind: Failure, elapsed: u32) {
        self.primary[at] = Report { kind, elapsed };
    }
    pub fn entry_fault(&mut self, at: usize, kind: Failure, elapsed: u32) {
        self.entry[at] = Report { kind, elapsed };
    }
    fn start(&self, clock: CpuClocks) {
        assert_eq!(clock.0, self.clock, "device clock ownership");
    }
    fn idle_at(&mut self, clock: CpuClocks, clocks: u32) {
        self.start(clock);
        self.events.push(Event::Idle(clock.0, clocks));
        self.counts[4] += 1;
        self.clock += u64::from(clocks);
    }
    fn mutate(&mut self, after: bool) {
        for mutation in self
            .mutations
            .into_iter()
            .flatten()
            .filter(|m| m.at == self.count && m.after == after)
        {
            self.words.insert(mutation.address, mutation.word);
            self.mutated += 1;
        }
    }
    fn instruction<T>(&mut self, value: T) -> InstructionReport<T, &'static str> {
        let report = self.primary[self.count];
        self.count += 1;
        self.counts[0] += 1;
        self.scripts.push(0, report.kind, report.elapsed);
        self.clock += u64::from(report.elapsed);
        InstructionReport {
            elapsed: report.elapsed,
            outcome: match report.kind {
                Failure::Complete => InstructionOutcome::Complete(value),
                Failure::Host => InstructionOutcome::HostFailure("host"),
                Failure::Boundary => InstructionOutcome::AtGroup0Entry,
                Failure::Unavailable => InstructionOutcome::TimingUnavailable,
            },
        }
    }
    fn entry_report<T>(&mut self, value: T) -> EntryReport<T, &'static str> {
        let report = self.entry[self.entry_count];
        self.entry_count += 1;
        self.counts[2] += 1;
        self.scripts.push(2, report.kind, report.elapsed);
        self.clock += u64::from(report.elapsed);
        Self::entry_outcome(report, value)
    }
    fn entry_outcome<T>(report: Report, value: T) -> EntryReport<T, &'static str> {
        EntryReport {
            elapsed: report.elapsed,
            outcome: match report.kind {
                Failure::Complete => EntryOutcome::Complete(value),
                Failure::Host => EntryOutcome::HostFailure("host"),
                Failure::Boundary => EntryOutcome::CriticalAbortReleased,
                Failure::Unavailable => EntryOutcome::TimingUnavailable,
            },
        }
    }
}
impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        a: BusAccess,
        clock: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.start(clock);
        self.mutate(false);
        let value = self.words.sample(a.bus_address);
        let report = self.primary[self.count];
        self.events.push(Event::Transfer {
            stage: 0,
            phase: Some(self.phases[self.count].unwrap_or_else(|| self.profile.phase(self.count))),
            access: a,
            direction: Direction::Read,
            value: Some(value),
            start: clock.0,
            elapsed: report.elapsed,
            kind: report.kind,
            deposited: false,
        });
        self.mutate(true);
        self.instruction(value)
    }
    fn write_instruction(
        &mut self,
        a: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.start(clock);
        self.mutate(false);
        let report = self.primary[self.count];
        let deposited = report.kind == Failure::Complete || self.primary_deposit[self.count];
        if deposited {
            let old = self.words.sample(a.bus_address);
            let word = match a.lanes {
                ByteLanes::Upper => old & 0x00ff | value & 0xff00,
                ByteLanes::Lower => old & 0xff00 | value & 0x00ff,
                ByteLanes::Both => value,
            };
            self.words.insert(a.bus_address, word);
        }
        self.events.push(Event::Transfer {
            stage: 0,
            phase: Some(self.phases[self.count].unwrap_or_else(|| self.profile.phase(self.count))),
            access: a,
            direction: Direction::Write,
            value: Some(value),
            start: clock.0,
            elapsed: report.elapsed,
            kind: report.kind,
            deposited,
        });
        self.mutate(true);
        self.instruction(())
    }
}
impl EntryBus for Device {
    type Error = &'static str;
    fn read_entry(&mut self, a: BusAccess, clock: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.start(clock);
        let value = self.words.sample(a.bus_address);
        let report = self.entry[self.entry_count];
        self.events.push(Event::Transfer {
            stage: 2,
            phase: None,
            access: a,
            direction: Direction::Read,
            value: Some(value),
            start: clock.0,
            elapsed: report.elapsed,
            kind: report.kind,
            deposited: false,
        });
        self.entry_report(value)
    }
    fn write_entry(
        &mut self,
        a: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.start(clock);
        let report = self.entry[self.entry_count];
        let deposited = report.kind == Failure::Complete || self.entry_deposit[self.entry_count];
        if deposited {
            self.words.insert(a.bus_address, value);
        }
        self.events.push(Event::Transfer {
            stage: 2,
            phase: None,
            access: a,
            direction: Direction::Write,
            value: Some(value),
            start: clock.0,
            elapsed: report.elapsed,
            kind: report.kind,
            deposited,
        });
        self.entry_report(())
    }
    fn idle_entry(&mut self, clock: CpuClocks, clocks: u32) {
        if clocks == 4 && self.before_entry.is_none() {
            self.before_entry = Some((clock.0, self.words.clone()));
        }
        self.idle_at(clock, clocks);
    }
    fn detect_address_error(
        &mut self,
        a: BusAccess,
        direction: Direction,
        clock: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.start(clock);
        let report = self.secondary;
        self.events.push(Event::Transfer {
            stage: 3,
            phase: None,
            access: a,
            direction,
            value: None,
            start: clock.0,
            elapsed: report.elapsed,
            kind: report.kind,
            deposited: false,
        });
        self.counts[3] += 1;
        self.scripts.push(3, report.kind, report.elapsed);
        self.clock += u64::from(report.elapsed);
        Self::entry_outcome(report, ())
    }
}
impl ClockedBus for Device {
    type Error = &'static str;
    fn read_cycle(&mut self, a: BusAccess, clock: CpuClocks) -> CycleResult<u16, Self::Error> {
        assert!(self.allow_legacy, "legacy read");
        self.start(clock);
        let value = self.words.sample(a.bus_address);
        self.events.push(Event::Transfer {
            stage: 4,
            phase: None,
            access: a,
            direction: Direction::Read,
            value: Some(value),
            start: clock.0,
            elapsed: 4,
            kind: Failure::Complete,
            deposited: false,
        });
        self.ordinary[0] += 1;
        self.clock += 4;
        CycleResult::ready(value)
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), Self::Error> {
        panic!("legacy write")
    }
    fn idle(&mut self, clock: CpuClocks, clocks: u32) {
        assert!(self.allow_legacy, "legacy idle");
        self.start(clock);
        self.events.push(Event::Idle(clock.0, clocks));
        self.ordinary[1] += 1;
        self.counts[4] += 1;
        self.clock += u64::from(clocks);
    }
}
pub(super) fn access(
    address: u32,
    bank: bool,
    program: bool,
    kind: AccessKind,
    byte: bool,
) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & zeff_m68k::bus::BUS_ADDRESS_MASK & !1,
        function_code: if program {
            if bank {
                FunctionCode::SupervisorProgram
            } else {
                FunctionCode::UserProgram
            }
        } else if bank {
            FunctionCode::SupervisorData
        } else {
            FunctionCode::UserData
        },
        kind,
        lanes: if byte {
            if address & 1 != 0 {
                ByteLanes::Lower
            } else {
                ByteLanes::Upper
            }
        } else {
            ByteLanes::Both
        },
    }
}
pub(super) struct DefaultSecondary<'a>(pub &'a mut Device);
impl EntryBus for DefaultSecondary<'_> {
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
impl InstructionBus for DefaultSecondary<'_> {
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

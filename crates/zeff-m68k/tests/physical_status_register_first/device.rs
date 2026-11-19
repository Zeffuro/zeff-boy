use super::*;
use model::{Case, Event, Outcome};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub counts: [usize; 5],
    pub before_entry: Option<(u64, BTreeMap<u32, u16>)>,
    script: Vec<(u32, Outcome, bool)>,
    next: usize,
    instruction_next: usize,
    entry_event: Option<usize>,
    mutations: Vec<(usize, u32, u16)>,
}

impl Device {
    pub fn reports_consumed(&self) -> bool {
        self.next == self.script.len()
    }
    pub fn new(c: &Case) -> Self {
        Self {
            clock: c.initial.clock,
            words: c.memory.iter().copied().collect(),
            events: Vec::new(),
            counts: [0; 5],
            before_entry: None,
            script: c
                .events
                .iter()
                .filter_map(|e| match *e {
                    Event::Transfer {
                        elapsed,
                        outcome,
                        deposited,
                        ..
                    } => Some((elapsed, outcome, deposited)),
                    Event::Idle(..) => None,
                })
                .collect(),
            next: 0,
            instruction_next: 0,
            entry_event: c.entry_event,
            mutations: c.mutations.clone(),
        }
    }
    fn prepare_instruction(&mut self) {
        for &(at, address, value) in &self.mutations {
            if at == self.instruction_next {
                self.words.insert(address & 16777214, value);
            }
        }
        self.instruction_next += 1;
    }
    fn sample(&self, access: BusAccess) -> u16 {
        self.words.get(&access.bus_address).copied().unwrap_or(0)
    }
    fn begin_entry(&mut self) {
        if self.before_entry.is_none() {
            self.before_entry = Some((self.clock, self.words.clone()));
        }
    }
    fn transfer(
        &mut self,
        stage: u8,
        access: BusAccess,
        direction: Direction,
        value: Option<u16>,
        start: CpuClocks,
    ) -> (u32, Outcome) {
        assert_eq!(self.clock, start.0);
        if stage >= 2 {
            self.begin_entry();
        }
        let (elapsed, outcome, deposit) =
            self.script
                .get(self.next)
                .copied()
                .unwrap_or((4, Outcome::Complete, false));
        let deposited = stage.is_multiple_of(2)
            && direction == Direction::Write
            && (outcome == Outcome::Complete || deposit);
        if deposited {
            self.words.insert(access.bus_address, value.unwrap());
        }
        self.events.push(Event::Transfer {
            stage,
            access,
            direction,
            value,
            start: start.0,
            elapsed,
            outcome,
            deposited,
        });
        self.counts[stage as usize] += 1;
        self.next += 1;
        self.clock += u64::from(elapsed);
        (elapsed, outcome)
    }
    fn instruction<T>(
        elapsed: u32,
        outcome: Outcome,
        value: T,
    ) -> InstructionReport<T, &'static str> {
        InstructionReport {
            elapsed,
            outcome: match outcome {
                Outcome::Complete => InstructionOutcome::Complete(value),
                Outcome::Boundary => InstructionOutcome::AtGroup0Entry,
                Outcome::Host => InstructionOutcome::HostFailure("host"),
                Outcome::Unavailable => InstructionOutcome::TimingUnavailable,
                Outcome::Critical => panic!("entry-only report"),
            },
        }
    }
    fn entry<T>(elapsed: u32, outcome: Outcome, value: T) -> EntryReport<T, &'static str> {
        EntryReport {
            elapsed,
            outcome: match outcome {
                Outcome::Complete => EntryOutcome::Complete(value),
                Outcome::Host => EntryOutcome::HostFailure("host"),
                Outcome::Unavailable => EntryOutcome::TimingUnavailable,
                Outcome::Critical => EntryOutcome::CriticalAbortReleased,
                Outcome::Boundary => panic!("instruction-only report"),
            },
        }
    }
}

impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.prepare_instruction();
        let value = self.sample(access);
        let (elapsed, outcome) = self.transfer(0, access, Direction::Read, Some(value), start);
        Self::instruction(elapsed, outcome, value)
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.prepare_instruction();
        let (elapsed, outcome) = self.transfer(0, access, Direction::Write, Some(value), start);
        Self::instruction(elapsed, outcome, ())
    }
    fn detect_instruction_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        start: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.prepare_instruction();
        let (elapsed, outcome) = self.transfer(1, access, direction, None, start);
        Self::instruction(elapsed, outcome, ())
    }
}
impl EntryBus for Device {
    type Error = &'static str;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, Self::Error> {
        let value = self.sample(access);
        let (elapsed, outcome) = self.transfer(2, access, Direction::Read, Some(value), start);
        Self::entry(elapsed, outcome, value)
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        let (elapsed, outcome) = self.transfer(2, access, Direction::Write, Some(value), start);
        Self::entry(elapsed, outcome, ())
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(self.clock, start.0);
        if self.entry_event == Some(self.events.len()) {
            self.begin_entry();
        }
        self.events.push(Event::Idle(start.0, clocks));
        self.counts[4] += 1;
        self.clock += u64::from(clocks);
    }
    fn detect_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        let (elapsed, outcome) = self.transfer(3, access, direction, None, start);
        Self::entry(elapsed, outcome, ())
    }
}
impl ClockedBus for Device {
    type Error = &'static str;
    fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, Self::Error> {
        panic!("ordinary read")
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), Self::Error> {
        panic!("ordinary write")
    }
    fn idle(&mut self, _: CpuClocks, _: u32) {
        panic!("ordinary idle")
    }
}

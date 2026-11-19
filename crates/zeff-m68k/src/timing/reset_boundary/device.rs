use crate::bus::{BusAccess, BusResponse, Direction};
use crate::timing::physical::{EntryBus, EntryOutcome, EntryReport};
use crate::timing::{ClockedBus, CpuClocks, CycleResult, InterruptVector};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u64),
    Write(BusAccess, u16, u64),
    Idle(u64, u32),
    Acknowledge(BusAccess, u64),
    Detect(BusAccess, Direction, u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub clock: u64,
    pub events: Vec<Event>,
    pub words: BTreeMap<u32, u16>,
    pub waits: u32,
    pub reads: usize,
    pub injection: Option<(usize, EntryOutcome<(), u8>)>,
}

impl Device {
    pub fn new(clock: u64, waits: u32) -> Self {
        Self {
            clock,
            events: Vec::new(),
            words: BTreeMap::from([
                (0, 0x00ff),
                (2, 0x2dc0),
                (4, 0),
                (6, 0x0300),
                (0x300, 0x4e71),
                (0x302, 0x4e71),
                (0x304, 0x4e71),
                (0x306, 0x4e71),
                (31 * 4, 0),
                (31 * 4 + 2, 0x0400),
                (0x400, 0x4e71),
                (0x402, 0x4e71),
                (0x404, 0x4e71),
            ]),
            waits,
            reads: 0,
            injection: None,
        }
    }
}

impl EntryBus for Device {
    type Error = u8;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, u8> {
        assert_eq!(self.clock, start.0);
        self.events.push(Event::Read(access, start.0));
        self.reads += 1;
        let mut elapsed = 4 + self.waits;
        let outcome = match self.injection.as_ref().filter(|(at, _)| *at == self.reads) {
            Some((_, EntryOutcome::Complete(()))) => {
                elapsed = 2;
                EntryOutcome::Complete(0)
            }
            Some((_, EntryOutcome::HostFailure(error))) => EntryOutcome::HostFailure(*error),
            Some((_, EntryOutcome::CriticalAbortReleased)) => EntryOutcome::CriticalAbortReleased,
            Some((_, EntryOutcome::TimingUnavailable)) => EntryOutcome::TimingUnavailable,
            None => EntryOutcome::Complete(*self.words.get(&access.bus_address).unwrap_or(&0x4e71)),
        };
        self.clock += u64::from(elapsed);
        EntryReport { elapsed, outcome }
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), u8> {
        let result = self.write_cycle(access, value, start);
        EntryReport {
            elapsed: 4 + result.wait_clocks,
            outcome: EntryOutcome::Complete(()),
        }
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.idle(start, clocks);
    }
    fn detect_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        start: CpuClocks,
    ) -> EntryReport<(), u8> {
        assert_eq!(self.clock, start.0);
        self.events.push(Event::Detect(access, direction, start.0));
        EntryReport {
            elapsed: 0,
            outcome: EntryOutcome::TimingUnavailable,
        }
    }
}

impl ClockedBus for Device {
    type Error = u8;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, u8> {
        assert_eq!(self.clock, start.0);
        self.events.push(Event::Read(access, start.0));
        self.clock += u64::from(4 + self.waits);
        CycleResult {
            result: Ok(*self.words.get(&access.bus_address).unwrap_or(&0x4e71)),
            wait_clocks: self.waits,
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), u8> {
        assert_eq!(self.clock, start.0);
        self.events.push(Event::Write(access, value, start.0));
        self.clock += u64::from(4 + self.waits);
        self.words.insert(access.bus_address, value);
        CycleResult {
            result: Ok(()),
            wait_clocks: self.waits,
        }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(self.clock, start.0);
        self.events.push(Event::Idle(start.0, clocks));
        self.clock += u64::from(clocks);
    }
    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, u8> {
        assert_eq!(self.clock, start.0);
        self.events.push(Event::Acknowledge(access, start.0));
        self.clock += u64::from(4 + self.waits);
        CycleResult {
            result: Ok(BusResponse::Complete(InterruptVector::Autovector)),
            wait_clocks: self.waits,
        }
    }
}

use std::collections::BTreeMap;
use zeff_m68k::bus::BusResponse;
use zeff_m68k::bus::{BUS_ADDRESS_MASK, BusAccess, ByteLanes};
use zeff_m68k::timing::InterruptVector;
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u64, u16, u32),
    Write(BusAccess, u64, u16, u32),
    Ack(BusAccess, u64, u32),
    Idle(u64, u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub waits: [u32; 16],
    pub fail: Option<usize>,
    pub count: usize,
    pub valid: bool,
}

impl Device {
    pub fn new(clock: u64) -> Self {
        Self {
            clock,
            words: BTreeMap::new(),
            events: Vec::new(),
            waits: [0; 16],
            fail: None,
            count: 0,
            valid: true,
        }
    }
    pub fn word(&self, logical: u32) -> u16 {
        self.words
            .get(&(logical & BUS_ADDRESS_MASK))
            .copied()
            .unwrap_or(0x4e71)
    }
    pub fn put(&mut self, logical: u32, value: u16) {
        self.words.insert(logical & BUS_ADDRESS_MASK, value);
    }
    fn cycle<T>(&mut self, start: CpuClocks, value: T) -> CycleResult<T, usize> {
        self.valid &= self.clock == start.0;
        let index = self.count;
        let wait = self.waits.get(index).copied().unwrap_or(0);
        self.count += 1;
        self.clock = self
            .clock
            .checked_add(4 + u64::from(wait))
            .unwrap_or_else(|| {
                self.valid = false;
                0
            });
        CycleResult {
            result: if self.fail == Some(index) {
                Err(index)
            } else {
                Ok(value)
            },
            wait_clocks: wait,
        }
    }
}

impl ClockedBus for Device {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        let value = self.word(access.bus_address);
        let result = self.cycle(start, value);
        self.events
            .push(Event::Read(access, start.0, value, result.wait_clocks));
        result
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        self.valid &= access.lanes == ByteLanes::Both;
        self.put(access.bus_address, value);
        let result = self.cycle(start, ());
        self.events
            .push(Event::Write(access, start.0, value, result.wait_clocks));
        result
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.valid &= self.clock == start.0;
        self.events.push(Event::Idle(start.0, clocks));
        self.clock = self
            .clock
            .checked_add(u64::from(clocks))
            .unwrap_or_else(|| {
                self.valid = false;
                0
            });
    }
    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, usize> {
        let result = self.cycle(start, BusResponse::Complete(InterruptVector::Autovector));
        self.events
            .push(Event::Ack(access, start.0, result.wait_clocks));
        result
    }
    fn begin_rmw(
        &mut self,
        _access: BusAccess,
        _start: CpuClocks,
    ) -> Result<BusResponse<()>, usize> {
        self.valid = false;
        Ok(BusResponse::Unsupported)
    }
    fn end_rmw(&mut self, _end: CpuClocks) {
        self.valid = false;
    }
    fn begin_reset_output(&mut self, _start: CpuClocks) -> Result<BusResponse<()>, usize> {
        self.valid = false;
        Ok(BusResponse::Unsupported)
    }
    fn end_reset_output(&mut self, _end: CpuClocks) {
        self.valid = false;
    }
}

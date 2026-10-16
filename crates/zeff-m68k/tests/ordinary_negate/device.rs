use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Event {
    pub access: BusAccess,
    pub at: u64,
    pub value: u16,
    pub direction: Direction,
    pub wait: u32,
    pub elapsed: u64,
    pub host: bool,
    pub deposited: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub waits: Vec<u32>,
    pub fail: Option<usize>,
    pub failed_write_deposits: Option<bool>,
    pub count: usize,
    pub valid: bool,
}

impl Device {
    pub fn new(c: &fixture::Case) -> Self {
        Self {
            clock: c.start,
            words: c.ram.iter().copied().collect(),
            events: Vec::new(),
            waits: c.waits.clone(),
            fail: c.fail,
            failed_write_deposits: c.failed_write_deposits,
            count: 0,
            valid: true,
        }
    }
    fn advance(
        &mut self,
        access: BusAccess,
        value: u16,
        direction: Direction,
        start: CpuClocks,
    ) -> (bool, bool, u32) {
        self.valid &= self.clock == start.0;
        let wait = self.waits.get(self.count).copied().unwrap_or(0);
        let host = self.fail == Some(self.count);
        let deposited =
            direction == Direction::Write && (!host || self.failed_write_deposits == Some(true));
        let elapsed = 4 + u64::from(wait);
        self.events.push(Event {
            access,
            at: start.0,
            value,
            direction,
            wait,
            elapsed,
            host,
            deposited,
        });
        self.count += 1;
        if let Some(clock) = self.clock.checked_add(elapsed) {
            self.clock = clock;
        } else {
            self.valid = false;
        }
        (host, deposited, wait)
    }
}

impl ClockedBus for Device {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        let value = self
            .words
            .get(&access.bus_address)
            .copied()
            .unwrap_or(0x4e71);
        let error = self.count;
        let (host, _, wait) = self.advance(access, value, Direction::Read, start);
        CycleResult {
            result: if host { Err(error) } else { Ok(value) },
            wait_clocks: wait,
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        let error = self.count;
        let (host, deposited, wait) = self.advance(access, value, Direction::Write, start);
        if deposited {
            let old = self.words.get(&access.bus_address).copied().unwrap_or(0);
            self.words.insert(
                access.bus_address,
                match access.lanes {
                    ByteLanes::Both => value,
                    ByteLanes::Upper => old & 0xff | value & 0xff00,
                    ByteLanes::Lower => old & 0xff00 | value & 0xff,
                },
            );
        }
        CycleResult {
            result: if host { Err(error) } else { Ok(()) },
            wait_clocks: wait,
        }
    }
    fn idle(&mut self, _: CpuClocks, _: u32) {
        panic!("unexpected ordinary NEG/NEGX idle")
    }
}

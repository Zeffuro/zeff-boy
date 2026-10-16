use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u64, u16, u32),
    Write(BusAccess, u64, u16, u32),
    Idle(u64, u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub waits: Vec<u32>,
    pub fail: Option<usize>,
    pub count: usize,
    pub valid: bool,
}

impl Device {
    pub fn new(case: &fixture::Case) -> Self {
        Self {
            clock: case.start,
            words: case.ram.iter().copied().collect(),
            events: Vec::new(),
            waits: case.waits.clone(),
            fail: case.fail,
            count: 0,
            valid: true,
        }
    }

    fn advance(&mut self, start: CpuClocks) -> (usize, u32) {
        self.valid &= self.clock == start.0;
        let index = self.count;
        let wait = self.waits.get(index).copied().unwrap_or(0);
        self.count += 1;
        if let Some(clock) = self.clock.checked_add(4 + u64::from(wait)) {
            self.clock = clock;
        } else {
            self.valid = false;
        }
        (index, wait)
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
        let (index, wait) = self.advance(start);
        self.events.push(Event::Read(access, start.0, value, wait));
        CycleResult {
            result: if self.fail == Some(index) {
                Err(index)
            } else {
                Ok(value)
            },
            wait_clocks: wait,
        }
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        let (index, wait) = self.advance(start);
        self.events.push(Event::Write(access, start.0, value, wait));
        let old = self.words.get(&access.bus_address).copied().unwrap_or(0);
        self.words.insert(
            access.bus_address,
            match access.lanes {
                ByteLanes::Both => value,
                ByteLanes::Upper => (old & 0xff) | (value & 0xff00),
                ByteLanes::Lower => (old & 0xff00) | (value & 0xff),
            },
        );
        CycleResult {
            result: if self.fail == Some(index) {
                Err(index)
            } else {
                Ok(())
            },
            wait_clocks: wait,
        }
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.valid &= self.clock == start.0;
        self.events.push(Event::Idle(start.0, clocks));
        if let Some(clock) = self.clock.checked_add(u64::from(clocks)) {
            self.clock = clock;
        } else {
            self.valid = false;
        }
    }
}

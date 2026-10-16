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
    pub waits: [u32; 12],
    pub fail: Option<usize>,
    pub count: usize,
    pub mutation: Option<(usize, u32, u16)>,
}
impl Device {
    pub fn new(clock: u64) -> Self {
        Self {
            clock,
            words: BTreeMap::new(),
            events: Vec::new(),
            waits: [0; 12],
            fail: None,
            count: 0,
            mutation: None,
        }
    }
    pub fn word(&mut self, address: u32, value: u16) {
        self.words.insert(address & 0x00ff_fffe, value);
    }
}
impl ClockedBus for Device {
    type Error = &'static str;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, Self::Error> {
        assert_eq!(self.clock, start.0);
        if let Some((at, address, value)) = self.mutation
            && at == self.count
        {
            self.word(address, value);
            self.mutation = None;
        }
        let value = self
            .words
            .get(&access.bus_address)
            .copied()
            .unwrap_or(0x4e71);
        let wait = self.waits[self.count];
        self.events.push(Event::Read(access, start.0, value, wait));
        let failed = self.fail == Some(self.count);
        self.count += 1;
        self.clock += 4 + u64::from(wait);
        CycleResult {
            result: if failed { Err("host") } else { Ok(value) },
            wait_clocks: wait,
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        assert_eq!(self.clock, start.0);
        let wait = self.waits[self.count];
        self.events.push(Event::Write(access, start.0, value, wait));
        let failed = self.fail == Some(self.count);
        if !failed {
            let old = self.words.get(&access.bus_address).copied().unwrap_or(0);
            self.words.insert(
                access.bus_address,
                match access.lanes {
                    ByteLanes::Upper => (old & 0xff) | (value & 0xff00),
                    ByteLanes::Lower => (old & 0xff00) | (value & 0xff),
                    ByteLanes::Both => value,
                },
            );
        }
        self.count += 1;
        self.clock += 4 + u64::from(wait);
        CycleResult {
            result: if failed { Err("host") } else { Ok(()) },
            wait_clocks: wait,
        }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(self.clock, start.0);
        self.events.push(Event::Idle(start.0, clocks));
        self.clock += u64::from(clocks);
    }
}

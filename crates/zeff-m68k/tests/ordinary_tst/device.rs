use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u64, u16, u32),
    Idle(u64, u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub words: BTreeMap<u32, u16>,
    pub clock: u64,
    pub events: Vec<Event>,
    pub count: usize,
    pub wait: u32,
    pub final_wait: Option<u32>,
    pub final_index: usize,
    pub fail: Option<usize>,
    pub mutation: Option<(usize, u32, u16)>,
}

impl Device {
    pub fn sample(&self, address: u32) -> u16 {
        self.words[&(address & BUS_ADDRESS_MASK & !1)]
    }
    pub fn response_wait(&self) -> u32 {
        if self.count == self.final_index {
            self.final_wait.unwrap_or(self.wait)
        } else {
            self.wait
        }
    }
}

impl ClockedBus for Device {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        assert_eq!(start.0, self.clock);
        if let Some((index, address, value)) = self.mutation
            && self.count == index
        {
            self.words.insert(address & BUS_ADDRESS_MASK & !1, value);
        }
        let value = self.sample(access.bus_address);
        let wait = self.response_wait();
        self.events.push(Event::Read(access, start.0, value, wait));
        self.clock += 4 + u64::from(wait);
        let index = self.count;
        self.count += 1;
        CycleResult {
            result: if self.fail == Some(index) {
                Err(index)
            } else {
                Ok(value)
            },
            wait_clocks: wait,
        }
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), usize> {
        panic!("TST write")
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(start.0, self.clock);
        self.events.push(Event::Idle(start.0, clocks));
        self.clock += u64::from(clocks);
    }
}

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u64),
    Idle(u64, u32),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub clock: u64,
    pub count: usize,
    pub waits: [u32; 5],
    pub fail: Option<usize>,
    pub mutate: Option<(usize, u16)>,
    pub valid: bool,
}
impl Device {
    pub fn new(case: Case, words: BTreeMap<u32, u16>) -> Self {
        Self {
            words,
            events: Vec::new(),
            clock: case.start,
            count: 0,
            waits: [case.wait; 5],
            fail: case.failure,
            mutate: case.mutate,
            valid: true,
        }
    }
}
impl ClockedBus for Device {
    type Error = usize;
    fn read_cycle(&mut self, a: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        self.valid &= self.clock == start.0;
        self.events.push(Event::Read(a, start.0));
        if let Some((_, word)) = self.mutate.filter(|(at, _)| *at == self.count) {
            put(&mut self.words, a.logical_address, word);
        }
        let at = self.count;
        self.count += 1;
        let wait = self.waits[at];
        self.clock += 4 + u64::from(wait);
        CycleResult {
            result: if self.fail == Some(at) {
                Err(at)
            } else {
                Ok(sample(&self.words, a.logical_address))
            },
            wait_clocks: wait,
        }
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), usize> {
        self.valid = false;
        CycleResult {
            result: Err(99),
            wait_clocks: 0,
        }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.valid &= self.clock == start.0;
        self.events.push(Event::Idle(start.0, clocks));
        self.clock += u64::from(clocks);
    }
}

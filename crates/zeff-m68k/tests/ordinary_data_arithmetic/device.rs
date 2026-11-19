use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u64, u16),
    Idle(u64, u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
    pub clock: u64,
    pub count: usize,
    pub waits: [u32; 5],
    pub failure: Option<usize>,
    pub mutate: Option<(usize, u16)>,
    pub valid: bool,
}

impl Device {
    pub(super) fn new(words: BTreeMap<u32, u16>, clock: u64, failure: Option<usize>) -> Self {
        Self {
            words,
            events: Vec::new(),
            clock,
            count: 0,
            waits: [WAIT; 5],
            failure,
            mutate: None,
            valid: true,
        }
    }
}

impl ClockedBus for Device {
    type Error = usize;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        let at = self.count;
        if let Some((_, value)) = self.mutate.filter(|(index, _)| *index == at) {
            put(&mut self.words, access.logical_address, value);
        }
        let word = sample(&self.words, access.logical_address);
        self.valid &= self.clock == start.0;
        self.events.push(Event::Read(access, start.0, word));
        self.count += 1;
        self.clock += 4 + u64::from(self.waits[at]);
        CycleResult {
            result: if self.failure == Some(at) {
                Err(at)
            } else {
                Ok(word)
            },
            wait_clocks: self.waits[at],
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

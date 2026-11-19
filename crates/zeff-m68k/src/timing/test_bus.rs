use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u64),
    Write(BusAccess, u16, u64),
    Ack(BusAccess, u64),
    Idle(u64, u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Bus {
    pub(super) words: BTreeMap<u32, u16>,
    pub(super) events: Vec<Event>,
    pub(super) clock: u64,
    pub(super) wait: u32,
    pub(super) response: Option<InterruptVector>,
    pub(super) transfers: usize,
    pub(super) fail: Option<usize>,
    pub(super) read_values: Vec<(usize, u16)>,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            words: BTreeMap::new(),
            events: Vec::new(),
            clock: 0,
            wait: 0,
            response: Some(InterruptVector::Autovector),
            transfers: 0,
            fail: None,
            read_values: Vec::new(),
        }
    }
}

impl Bus {
    fn result<T>(&mut self, value: T, start: CpuClocks) -> CycleResult<T, usize> {
        assert_eq!(start.0, self.clock);
        self.clock += 4 + u64::from(self.wait);
        let index = self.transfers;
        self.transfers += 1;
        CycleResult {
            result: if self.fail == Some(index) {
                Err(index)
            } else {
                Ok(value)
            },
            wait_clocks: self.wait,
        }
    }
    pub(super) fn handler(&mut self, vector: u8) {
        self.words.insert(u32::from(vector) * 4, 0x1234);
        self.words.insert(u32::from(vector) * 4 + 2, 0x5000);
        self.words.insert(0x345000, 0x4e71);
        self.words.insert(0x345002, 0x7201);
    }
}

impl ClockedBus for Bus {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        self.events.push(Event::Read(access, start.0));
        let value = self
            .read_values
            .iter()
            .find(|(index, _)| *index == self.transfers)
            .map(|(_, value)| *value)
            .unwrap_or_else(|| *self.words.get(&access.bus_address).unwrap_or(&0x4e71));
        self.result(value, start)
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        self.events.push(Event::Write(access, value, start.0));
        let response = self.result((), start);
        if response.result.is_ok() {
            let old = *self.words.get(&access.bus_address).unwrap_or(&0);
            let value = match access.lanes {
                ByteLanes::Upper => (old & 0xff) | (value & 0xff00),
                ByteLanes::Lower => (old & 0xff00) | (value & 0xff),
                ByteLanes::Both => value,
            };
            self.words.insert(access.bus_address, value);
        }
        response
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(start.0, self.clock);
        self.events.push(Event::Idle(start.0, clocks));
        self.clock += u64::from(clocks);
    }
    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, usize> {
        self.events.push(Event::Ack(access, start.0));
        match self.response {
            Some(response) => self.result(BusResponse::Complete(response), start),
            None => CycleResult::ready(BusResponse::Unsupported),
        }
    }
}

pub(super) fn cpu(sr: u16) -> PrefetchCpu {
    let mut regs = Registers::new();
    regs.set_status_register(sr);
    regs.pc = 0x1234_0100;
    regs.ssp = 0x1000;
    regs.usp = 0x2000;
    PrefetchCpu::from_prefetched(regs, [0x4e71, 0x4e71], CpuClocks(0)).unwrap()
}

use std::collections::BTreeMap;
use zeff_m68k::bus::{BUS_ADDRESS_MASK, BusAccess, ByteLanes};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult};

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
    pub deposit_failed: bool,
    pub mutation: Option<(usize, u32, u16)>,
    pub count: usize,
    pub valid: bool,
}

impl Device {
    pub fn new(clock: u64) -> Self {
        Self {
            clock,
            words: BTreeMap::new(),
            events: Vec::new(),
            waits: [0; 12],
            fail: None,
            deposit_failed: true,
            mutation: None,
            count: 0,
            valid: true,
        }
    }

    pub fn word(&mut self, address: u32, value: u16) {
        self.words.insert(address & BUS_ADDRESS_MASK & !1, value);
    }

    fn before(&mut self, start: CpuClocks) -> (usize, u32, bool) {
        self.valid &= self.clock == start.0;
        if let Some((at, address, value)) = self.mutation
            && at == self.count
        {
            self.word(address, value);
            self.mutation = None;
        }
        let index = self.count;
        let wait = self.waits.get(index).copied().unwrap_or(0);
        self.count += 1;
        self.clock = self.clock.checked_add(4 + u64::from(wait)).unwrap();
        (index, wait, self.fail == Some(index))
    }
}

impl ClockedBus for Device {
    type Error = usize;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        let (index, wait, failed) = self.before(start);
        let value = self
            .words
            .get(&access.bus_address)
            .copied()
            .unwrap_or(0x4e71);
        self.events.push(Event::Read(access, start.0, value, wait));
        CycleResult {
            result: if failed { Err(index) } else { Ok(value) },
            wait_clocks: wait,
        }
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        let (index, wait, failed) = self.before(start);
        self.events.push(Event::Write(access, start.0, value, wait));
        if !failed || self.deposit_failed {
            let old = self.words.get(&access.bus_address).copied().unwrap_or(0);
            let value = match access.lanes {
                ByteLanes::Both => value,
                ByteLanes::Upper => old & 0xff | value & 0xff00,
                ByteLanes::Lower => old & 0xff00 | value & 0xff,
            };
            self.words.insert(access.bus_address, value);
        }
        CycleResult {
            result: if failed { Err(index) } else { Ok(()) },
            wait_clocks: wait,
        }
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.valid &= self.clock == start.0;
        self.events.push(Event::Idle(start.0, clocks));
        self.clock = self.clock.checked_add(u64::from(clocks)).unwrap();
    }
}

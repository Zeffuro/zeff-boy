use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Read(BusAccess, u64, u16, u32),
    Write(BusAccess, u64, u16, u32),
    Idle(u64, u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub words: [u16; WORDS],
    pub clock: u64,
    pub count: usize,
    pub events: [Event; EVENTS],
    pub length: usize,
    pub host: Option<usize>,
    pub maximum: Option<usize>,
}

impl Device {
    pub fn new() -> Self {
        Self {
            words: [0; WORDS],
            clock: 0,
            count: 0,
            events: [Event::Idle(0, 0); EVENTS],
            length: 0,
            host: None,
            maximum: None,
        }
    }

    pub fn word(&mut self, address: u32, value: u16) {
        self.words[(address as usize & MEMORY_MASK) / 2] = value;
    }

    pub fn push(&mut self, event: Event) {
        self.events[self.length] = event;
        self.length += 1;
    }

    fn advance(&mut self, at: CpuClocks) -> (usize, u32) {
        assert_eq!(at.0, self.clock);
        let index = self.count;
        self.count += 1;
        let wait = if self.maximum == Some(index) {
            u32::MAX
        } else {
            0
        };
        self.clock += 4 + u64::from(wait);
        (index, wait)
    }
}

impl ClockedBus for Device {
    type Error = usize;

    fn read_cycle(&mut self, access: BusAccess, at: CpuClocks) -> CycleResult<u16, usize> {
        let value = self.words[(access.bus_address as usize & MEMORY_MASK) / 2];
        let (index, wait) = self.advance(at);
        self.push(Event::Read(access, at.0, value, wait));
        CycleResult {
            result: if self.host == Some(index) {
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
        at: CpuClocks,
    ) -> CycleResult<(), usize> {
        assert_eq!(access.lanes, ByteLanes::Both);
        let (index, wait) = self.advance(at);
        self.push(Event::Write(access, at.0, value, wait));
        self.word(access.bus_address, value);
        CycleResult {
            result: if self.host == Some(index) {
                Err(index)
            } else {
                Ok(())
            },
            wait_clocks: wait,
        }
    }

    fn idle(&mut self, at: CpuClocks, clocks: u32) {
        assert_eq!(at.0, self.clock);
        self.push(Event::Idle(at.0, clocks));
        self.clock += u64::from(clocks);
    }
}

use zeff_m68k::bus::{BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixed<T: Copy> {
    pub slots: [Option<T>; 48],
    pub len: usize,
}

impl<T: Copy> Default for Fixed<T> {
    fn default() -> Self {
        Self {
            slots: [None; 48],
            len: 0,
        }
    }
}

impl<T: Copy> Fixed<T> {
    pub fn push(&mut self, value: T) {
        self.slots[self.len] = Some(value);
        self.len += 1;
    }
}

impl Fixed<(u32, u16)> {
    pub fn sample(&self, address: u32) -> u16 {
        let key = address & BUS_ADDRESS_MASK & !1;
        self.slots[..self.len]
            .iter()
            .flatten()
            .find(|(old, _)| *old == key)
            .map_or(0, |(_, value)| *value)
    }

    pub fn insert(&mut self, address: u32, value: u16) {
        let key = address & BUS_ADDRESS_MASK & !1;
        if let Some(slot) = self.slots[..self.len]
            .iter_mut()
            .find(|slot| slot.is_some_and(|(old, _)| old == key))
        {
            *slot = Some((key, value));
        } else {
            self.push((key, value));
        }
    }

    pub fn deposit(&mut self, access: BusAccess, value: u16) {
        let old = self.sample(access.bus_address);
        let value = match access.lanes {
            ByteLanes::Upper => old & 0x00ff | value & 0xff00,
            ByteLanes::Lower => old & 0xff00 | value & 0x00ff,
            ByteLanes::Both => value,
        };
        self.insert(access.bus_address, value);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Transfer {
        phase: &'static str,
        access: BusAccess,
        direction: Direction,
        value: u16,
        start: u64,
        wait: u32,
        elapsed: u64,
        failed: bool,
        deposited: bool,
        index: usize,
    },
    Idle {
        phase: &'static str,
        start: u64,
        clocks: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Script {
    pub phase: &'static str,
    pub direction: Direction,
    pub wait: u32,
    pub failure: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub words: Fixed<(u32, u16)>,
    pub events: Fixed<Event>,
    pub scripts: Fixed<Script>,
    pub clock: u64,
    pub transfers: usize,
    pub valid: bool,
    pub phases: [Option<&'static str>; 6],
    pub waits: [u32; 6],
    pub phase_count: usize,
    pub fail: Option<usize>,
    pub failed_write_deposit: bool,
    pub idle_phase: &'static str,
}

impl Device {
    fn callback(
        &mut self,
        access: BusAccess,
        direction: Direction,
        value: u16,
        start: CpuClocks,
    ) -> (usize, bool, u32) {
        self.valid &= start.0 == self.clock && self.transfers < self.phase_count;
        let index = self.transfers;
        let phase = self.phases.get(index).copied().flatten().unwrap_or("Extra");
        let wait = self.waits.get(index).copied().unwrap_or(0);
        let failed = self.fail == Some(index);
        let deposited = direction == Direction::Write && (!failed || self.failed_write_deposit);
        if deposited {
            self.words.deposit(access, value);
        }
        let elapsed = 4 + u64::from(wait);
        self.events.push(Event::Transfer {
            phase,
            access,
            direction,
            value,
            start: start.0,
            wait,
            elapsed,
            failed,
            deposited,
            index,
        });
        self.scripts.push(Script {
            phase,
            direction,
            wait,
            failure: failed.then_some(index),
        });
        self.clock = self.clock.checked_add(elapsed).unwrap();
        self.transfers += 1;
        (index, failed, wait)
    }
}

impl ClockedBus for Device {
    type Error = usize;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        let value = self.words.sample(access.bus_address);
        let (index, failed, wait) = self.callback(access, Direction::Read, value, start);
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
        let (index, failed, wait) = self.callback(access, Direction::Write, value, start);
        CycleResult {
            result: if failed { Err(index) } else { Ok(()) },
            wait_clocks: wait,
        }
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.valid &= start.0 == self.clock;
        self.events.push(Event::Idle {
            phase: self.idle_phase,
            start: start.0,
            clocks,
        });
        self.clock = self.clock.checked_add(u64::from(clocks)).unwrap();
    }
}

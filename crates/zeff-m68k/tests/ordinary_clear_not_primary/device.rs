use crate::fixed::Fixed;
use zeff_m68k::bus::{BUS_ADDRESS_MASK, BusAccess, Direction};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult};

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
    pub access: BusAccess,
    pub direction: Direction,
    pub wait: u32,
    pub result: Result<Option<u16>, usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mutation {
    pub phase: &'static str,
    pub after: bool,
    pub address: u32,
    pub value: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    pub mutation: Mutation,
    pub before: u16,
    pub clock: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub words: Fixed<(u32, u16)>,
    pub events: Fixed<Event>,
    pub scripts: Fixed<Script>,
    pub effects: Fixed<Effect>,
    pub clock: u64,
    pub transfers: usize,
    pub valid: bool,
    pub phases: [Option<&'static str>; 48],
    pub waits: [u32; 48],
    pub phase_count: usize,
    pub fail: Option<&'static str>,
    pub failed_write_deposit: bool,
    pub idle_phase: &'static str,
    pub mutation: Option<Mutation>,
}

impl Device {
    pub fn new(clock: u64) -> Self {
        Self {
            words: Fixed::default(),
            events: Fixed::default(),
            scripts: Fixed::default(),
            effects: Fixed::default(),
            clock,
            transfers: 0,
            valid: true,
            phases: [None; 48],
            waits: [0; 48],
            phase_count: 0,
            fail: None,
            failed_write_deposit: false,
            idle_phase: "InstructionI2",
            mutation: None,
        }
    }

    pub fn mutate(&mut self, phase: &'static str, after: bool) {
        if let Some(change) = self
            .mutation
            .filter(|change| change.phase == phase && change.after == after)
        {
            let before = self.words.sample(change.address);
            self.words.insert(change.address, change.value);
            self.effects.push(Effect {
                mutation: change,
                before,
                clock: self.clock,
            });
        }
    }

    fn callback(
        &mut self,
        access: BusAccess,
        direction: Direction,
        payload: u16,
        start: CpuClocks,
    ) -> (u16, bool, u32, usize) {
        self.valid &= start.0 == self.clock && self.transfers < self.phase_count;
        let index = self.transfers;
        let phase = self.phases[index].unwrap_or("Extra");
        let wait = self.waits[index];
        let failed = self.fail == Some(phase);
        self.mutate(phase, false);
        let sampled = self.words.sample(access.bus_address);
        let deposited = direction == Direction::Write && (!failed || self.failed_write_deposit);
        if deposited {
            self.words.deposit(access, payload);
        }
        self.events.push(Event::Transfer {
            phase,
            access,
            direction,
            value: if direction == Direction::Write {
                payload
            } else {
                sampled
            },
            start: self.clock,
            wait,
            elapsed: 4 + u64::from(wait),
            failed,
            deposited,
            index,
        });
        self.scripts.push(Script {
            phase,
            access,
            direction,
            wait,
            result: if failed {
                Err(index)
            } else {
                Ok((direction == Direction::Read).then_some(sampled))
            },
        });
        self.mutate(phase, true);
        self.clock = self.clock.checked_add(4 + u64::from(wait)).unwrap();
        self.transfers += 1;
        (sampled, failed, wait, index)
    }

    pub fn mutation_bus_address(change: Mutation) -> u32 {
        change.address & BUS_ADDRESS_MASK & !1
    }
}

impl ClockedBus for Device {
    type Error = usize;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        let (value, failed, wait, index) = self.callback(access, Direction::Read, 0, start);
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
        let (_, failed, wait, index) = self.callback(access, Direction::Write, value, start);
        CycleResult {
            result: if failed { Err(index) } else { Ok(()) },
            wait_clocks: wait,
        }
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.valid &= start.0 == self.clock;
        self.events.push(Event::Idle {
            phase: self.idle_phase,
            start: self.clock,
            clocks,
        });
        self.clock = self.clock.checked_add(u64::from(clocks)).unwrap();
    }
}

use crate::fixed::Fixed;
use zeff_m68k::bus::{BUS_ADDRESS_MASK, BusAccess, BusResponse, Direction};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, InterruptVector};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Unsupported,
    Autovector,
    Spurious,
    Vector64,
    Host,
}

impl Reply {
    pub fn parse(name: &str) -> Self {
        match name {
            "Unsupported" => Self::Unsupported,
            "Autovector" => Self::Autovector,
            "Spurious" => Self::Spurious,
            "Vector64" => Self::Vector64,
            "Host" => Self::Host,
            _ => panic!("reply"),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Unsupported => "Unsupported",
            Self::Autovector => "Autovector",
            Self::Spurious => "Spurious",
            Self::Vector64 => "Vector64",
            Self::Host => "Host",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Datum {
    Word(u16),
    Reply(Reply),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Transfer {
        phase: &'static str,
        access: BusAccess,
        direction: Direction,
        value: Datum,
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
    pub result: Result<Option<Datum>, usize>,
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
    pub wait_config: Fixed<(&'static str, u32)>,
    pub primary_count: usize,
    pub primary_idle_remaining: usize,
    pub entry_mode: Option<bool>,
    pub entry_idle_count: usize,
    pub entry_fail: Option<&'static str>,
    pub entry_failed_deposit: bool,
    pub reply: Reply,
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
            wait_config: Fixed::default(),
            primary_count: 0,
            primary_idle_remaining: 0,
            entry_mode: None,
            entry_idle_count: 0,
            entry_fail: None,
            entry_failed_deposit: false,
            reply: Reply::Autovector,
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
        let failed = self.fail == Some(phase) || self.entry_fail == Some(phase);
        self.mutate(phase, false);
        let sampled = self.words.sample(access.bus_address);
        let deposition = if index < self.primary_count {
            self.failed_write_deposit
        } else {
            self.entry_failed_deposit
        };
        let deposited = direction == Direction::Write && (!failed || deposition);
        if deposited {
            self.words.deposit(access, payload);
        }
        self.events.push(Event::Transfer {
            phase,
            access,
            direction,
            value: Datum::Word(if direction == Direction::Write {
                payload
            } else {
                sampled
            }),
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
                Ok((direction == Direction::Read).then_some(Datum::Word(sampled)))
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

    pub fn entry_idle_phase(&self) -> &'static str {
        match (self.entry_mode.unwrap(), self.entry_idle_count) {
            (true, 0) => "TraceI4",
            (true, 1) => "TraceI2",
            (false, 0) => "InterruptI6",
            (false, 1) => "InterruptI4",
            (false, 2) => "InterruptI2",
            _ => panic!("idle phase"),
        }
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
        let phase = if self.primary_idle_remaining > 0 {
            self.primary_idle_remaining -= 1;
            self.idle_phase
        } else {
            let phase = self.entry_idle_phase();
            self.entry_idle_count += 1;
            phase
        };
        self.events.push(Event::Idle {
            phase,
            start: self.clock,
            clocks,
        });
        self.clock = self.clock.checked_add(u64::from(clocks)).unwrap();
    }

    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, usize> {
        self.valid &= start.0 == self.clock && self.transfers < self.phase_count;
        let index = self.transfers;
        let phase = self.phases[index].unwrap();
        let wait = self.waits[index];
        let failed = self.reply == Reply::Host || self.entry_fail == Some(phase);
        self.events.push(Event::Transfer {
            phase,
            access,
            direction: Direction::Read,
            value: Datum::Reply(self.reply),
            start: self.clock,
            wait,
            elapsed: 4 + u64::from(wait),
            failed,
            deposited: false,
            index,
        });
        self.scripts.push(Script {
            phase,
            access,
            direction: Direction::Read,
            wait,
            result: if failed {
                Err(index)
            } else {
                Ok(Some(Datum::Reply(self.reply)))
            },
        });
        self.clock = self.clock.checked_add(4 + u64::from(wait)).unwrap();
        self.transfers += 1;
        CycleResult {
            wait_clocks: wait,
            result: if failed {
                Err(index)
            } else {
                Ok(match self.reply {
                    Reply::Unsupported => BusResponse::Unsupported,
                    Reply::Autovector => BusResponse::Complete(InterruptVector::Autovector),
                    Reply::Spurious => BusResponse::Complete(InterruptVector::Spurious),
                    Reply::Vector64 => BusResponse::Complete(InterruptVector::Vector(64)),
                    Reply::Host => unreachable!(),
                })
            },
        }
    }
}

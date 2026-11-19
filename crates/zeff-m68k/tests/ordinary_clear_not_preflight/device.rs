use crate::fixed::Fixed;
use crate::preflight_axes::{EXTRA, State, TRACE};
use zeff_m68k::bus::{BUS_ADDRESS_MASK, BusAccess, Direction};
use zeff_m68k::timing::physical::{EntryBus, EntryOutcome, EntryReport};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionOutcome, InstructionReport,
};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportKind {
    Complete,
    TimingUnavailable,
    CriticalAbortReleased,
}
impl ReportKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Complete => "Complete",
            Self::TimingUnavailable => "TimingUnavailable",
            Self::CriticalAbortReleased => "CriticalAbortReleased",
        }
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
    Physical {
        phase: &'static str,
        access: BusAccess,
        start: u64,
        elapsed: u32,
        value: u16,
        outcome: ReportKind,
        instruction: bool,
        index: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Ordinary {
        phase: &'static str,
        access: BusAccess,
        direction: Direction,
        wait: u32,
        result: Result<Option<u16>, usize>,
    },
    Physical {
        phase: &'static str,
        access: BusAccess,
        elapsed: u32,
        outcome: ReportKind,
        instruction: bool,
    },
}
impl Script {
    pub fn phase(self) -> &'static str {
        match self {
            Self::Ordinary { phase, .. } | Self::Physical { phase, .. } => phase,
        }
    }
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
    pub state: State,
    pub probe: bool,
    pub probe_transfers: usize,
    pub probe_idles: usize,
    pub reset_transfers: usize,
    pub reset_detectors: usize,
    pub reset_defaults: Fixed<()>,
    pub reset_reports: [Option<(ReportKind, u32)>; 48],
    pub physical_reports: [Option<(ReportKind, u32)>; 48],
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
            state: State::PCMismatch,
            probe: false,
            probe_transfers: 0,
            probe_idles: 0,
            reset_transfers: 0,
            reset_detectors: 0,
            reset_defaults: Fixed::default(),
            reset_reports: [None; 48],
            physical_reports: [None; 48],
        }
    }

    pub fn configure(&mut self, state: State) {
        self.state = state;
        assert!(!self.probe);
        for name in EXTRA {
            self.phases[self.phase_count] = Some(name);
            self.phase_count += 1;
        }
        self.reset_reports[0] = Some((ReportKind::CriticalAbortReleased, 8));
        for slot in &mut self.reset_reports[1..6] {
            *slot = Some((ReportKind::Complete, 4));
        }
        self.physical_reports[0] = Some((ReportKind::TimingUnavailable, 5));
    }

    pub fn begin_probe(&mut self) {
        assert!(!self.probe);
        self.probe = true;
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

    pub fn mutation_bus_address(change: Mutation) -> u32 {
        change.address & BUS_ADDRESS_MASK & !1
    }

    pub fn ordinary(
        &mut self,
        access: BusAccess,
        direction: Direction,
        payload: u16,
        start: CpuClocks,
    ) -> (u16, bool, u32, usize) {
        let index = self.transfers;
        self.valid &= start.0 == self.clock;
        let phase = if self.probe {
            if self.probe_idles == 0 {
                ["ProbeFetch0", "ProbeFetch1"][self.probe_transfers]
            } else {
                TRACE[self.probe_transfers]
            }
        } else {
            self.phases[index].unwrap()
        };
        let position = self.phases[..self.phase_count]
            .iter()
            .position(|name| *name == Some(phase))
            .unwrap();
        let wait = self.waits[position];
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
        self.scripts.push(Script::Ordinary {
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
        if self.probe {
            self.probe_transfers += 1;
        }
        (sampled, failed, wait, index)
    }

    pub fn pause(&mut self, phase: &'static str, clocks: u32, start: CpuClocks) {
        self.valid &= start.0 == self.clock;
        self.events.push(Event::Idle {
            phase,
            start: self.clock,
            clocks,
        });
        self.clock = self.clock.checked_add(u64::from(clocks)).unwrap();
    }

    pub fn physical(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        instruction: bool,
    ) -> (ReportKind, u32) {
        self.valid &= start.0 == self.clock && !self.probe;
        let (phase, index, report) = if instruction {
            (
                "SetupPhysical",
                self.transfers,
                self.physical_reports[0].unwrap(),
            )
        } else {
            (
                "ResetSPHigh",
                self.reset_transfers,
                self.reset_reports[self.reset_transfers].unwrap(),
            )
        };
        let (outcome, elapsed) = report;
        let value = self.words.sample(access.bus_address);
        self.events.push(Event::Physical {
            phase,
            access,
            start: self.clock,
            elapsed,
            value,
            outcome,
            instruction,
            index,
        });
        self.scripts.push(Script::Physical {
            phase,
            access,
            elapsed,
            outcome,
            instruction,
        });
        self.clock = self.clock.checked_add(u64::from(elapsed)).unwrap();
        self.transfers += 1;
        if !instruction {
            self.reset_transfers += 1;
        }
        report
    }
}

impl ClockedBus for Device {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        let (value, failed, wait, index) = self.ordinary(access, Direction::Read, 0, start);
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
        let (_, failed, wait, index) = self.ordinary(access, Direction::Write, value, start);
        CycleResult {
            result: if failed { Err(index) } else { Ok(()) },
            wait_clocks: wait,
        }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        let phase = if self.probe {
            if self.probe_idles == 0 {
                "TraceI4"
            } else {
                "TraceI2"
            }
        } else if self.state == State::Stopped {
            "SetupStopI4"
        } else {
            self.idle_phase
        };
        self.pause(phase, clocks, start);
        if self.probe {
            self.probe_idles += 1;
        }
    }
}

impl EntryBus for Device {
    type Error = usize;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, usize> {
        let (outcome, elapsed) = self.physical(access, start, false);
        EntryReport {
            elapsed,
            outcome: match outcome {
                ReportKind::Complete => {
                    EntryOutcome::Complete(self.words.sample(access.bus_address))
                }
                ReportKind::TimingUnavailable => EntryOutcome::TimingUnavailable,
                ReportKind::CriticalAbortReleased => EntryOutcome::CriticalAbortReleased,
            },
        }
    }
    fn write_entry(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> EntryReport<(), usize> {
        panic!("unexpected setup entry write")
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        let phase = if self.state == State::PhysicalUnqualified {
            "SetupPhysicalI2"
        } else {
            "ResetI14"
        };
        self.pause(phase, clocks, start);
    }
}

impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, usize> {
        let (outcome, elapsed) = self.physical(access, start, true);
        assert_eq!(outcome, ReportKind::TimingUnavailable);
        InstructionReport {
            elapsed,
            outcome: InstructionOutcome::TimingUnavailable,
        }
    }
    fn write_instruction(
        &mut self,
        _: BusAccess,
        _: u16,
        _: CpuClocks,
    ) -> InstructionReport<(), usize> {
        panic!("unexpected setup instruction write")
    }
}

use std::collections::BTreeMap;

use zeff_m68k::bus::{
    AccessKind, BUS_ADDRESS_MASK, BusAccess, BusResponse, ByteLanes, Direction, FunctionCode,
};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryInputError, EntryKind, EntryOutcome, EntryReport,
    ExternalResetBoundary, Group0Capture, ImportedGroup0Boundary, PhysicalEntryState,
    PhysicalInspection, ProcessingClass, ResetQualification, SswProfile, TimingQualification,
};
use zeff_m68k::timing::{
    ClockedBus, CpuClocks, CycleResult, InterruptVector, PrefetchCpu, TimingFault,
};
use zeff_m68k::{Registers, SUPERVISOR, TRACE};

#[path = "physical_entry/group0.rs"]
mod group0;
#[path = "physical_entry/reset.rs"]
mod reset;
#[path = "physical_entry/terminal.rs"]
mod terminal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Read(BusAccess, u64),
    Write(BusAccess, u16, u64),
    Idle(u64, u32),
    Detect(BusAccess, Direction, u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Injection {
    Host,
    Released,
    Unavailable,
    ShortComplete,
}

struct Device {
    clock: u64,
    events: Vec<Event>,
    words: BTreeMap<u32, u16>,
    elapsed: Vec<u32>,
    transfers: usize,
    inject: Option<(usize, Injection)>,
    detect_supported: bool,
    detect_elapsed: u32,
    detect_outcome: Option<Injection>,
    ownership_edges: usize,
}

impl Device {
    fn new(clock: u64) -> Self {
        Self {
            clock,
            events: Vec::new(),
            words: BTreeMap::new(),
            elapsed: vec![4; 11],
            transfers: 0,
            inject: None,
            detect_supported: true,
            detect_elapsed: 9,
            detect_outcome: Some(Injection::Released),
            ownership_edges: 0,
        }
    }

    fn word(&mut self, logical: u32, value: u16) {
        self.words.insert(logical & BUS_ADDRESS_MASK, value);
    }

    fn handler(&mut self, vector: u8, address: u32) {
        self.word(u32::from(vector) * 4, (address >> 16) as u16);
        self.word(u32::from(vector) * 4 + 2, address as u16);
        self.word(address, 0x70a5);
        self.word(address.wrapping_add(2), 0x4e71);
        self.word(address.wrapping_add(4), 0x7203);
    }

    fn reset_vectors(&mut self, sp: u32, handler: u32) {
        self.word(0, (sp >> 16) as u16);
        self.word(2, sp as u16);
        self.handler(1, handler);
    }

    fn begin(&mut self, start: CpuClocks) {
        assert_eq!(start.0, self.clock, "device clock ownership");
    }

    fn idle_at(&mut self, start: CpuClocks, clocks: u32) {
        self.begin(start);
        self.events.push(Event::Idle(start.0, clocks));
        self.clock += u64::from(clocks);
    }

    fn selected(&mut self) -> (u32, Option<Injection>) {
        let index = self.transfers;
        self.transfers += 1;
        let injection = self
            .inject
            .filter(|(at, _)| *at == index)
            .map(|(_, kind)| kind);
        let elapsed = if injection == Some(Injection::ShortComplete) {
            3
        } else {
            self.elapsed.get(index).copied().unwrap_or(4)
        };
        self.clock += u64::from(elapsed);
        (elapsed, injection)
    }

    fn transfers(&self) -> Vec<(BusAccess, Option<u16>, u64)> {
        self.events
            .iter()
            .filter_map(|event| match *event {
                Event::Read(access, start) => Some((access, None, start)),
                Event::Write(access, value, start) => Some((access, Some(value), start)),
                _ => None,
            })
            .collect()
    }
}

impl ClockedBus for Device {
    type Error = &'static str;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, Self::Error> {
        self.begin(start);
        self.events.push(Event::Read(access, start.0));
        let (elapsed, injection) = self.selected();
        assert!(elapsed >= 4);
        CycleResult {
            result: if injection == Some(Injection::Host) {
                Err("host")
            } else {
                Ok(self
                    .words
                    .get(&access.bus_address)
                    .copied()
                    .unwrap_or(0x4e71))
            },
            wait_clocks: elapsed - 4,
        }
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        self.begin(start);
        self.events.push(Event::Write(access, value, start.0));
        self.word(access.logical_address, value);
        let (elapsed, injection) = self.selected();
        assert!(elapsed >= 4);
        CycleResult {
            result: if injection == Some(Injection::Host) {
                Err("host")
            } else {
                Ok(())
            },
            wait_clocks: elapsed - 4,
        }
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.idle_at(start, clocks);
    }

    fn begin_reset_output(&mut self, _: CpuClocks) -> Result<BusResponse<()>, Self::Error> {
        self.ownership_edges += 1;
        Ok(BusResponse::Complete(()))
    }

    fn begin_rmw(&mut self, _: BusAccess, _: CpuClocks) -> Result<BusResponse<()>, Self::Error> {
        self.ownership_edges += 1;
        Ok(BusResponse::Complete(()))
    }

    fn acknowledge_cycle(
        &mut self,
        _: BusAccess,
        _: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, Self::Error> {
        panic!("unexpected interrupt acknowledge")
    }
}

impl EntryBus for Device {
    type Error = &'static str;

    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.begin(start);
        self.events.push(Event::Read(access, start.0));
        let value = self
            .words
            .get(&access.bus_address)
            .copied()
            .unwrap_or(0x4e71);
        let (elapsed, injection) = self.selected();
        EntryReport {
            elapsed,
            outcome: outcome(value, injection),
        }
    }

    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.begin(start);
        self.events.push(Event::Write(access, value, start.0));
        self.word(access.logical_address, value);
        let (elapsed, injection) = self.selected();
        EntryReport {
            elapsed,
            outcome: outcome((), injection),
        }
    }

    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.idle_at(start, clocks);
    }

    fn detect_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.begin(start);
        if !self.detect_supported {
            return EntryReport {
                elapsed: 0,
                outcome: EntryOutcome::TimingUnavailable,
            };
        }
        self.events.push(Event::Detect(access, direction, start.0));
        self.clock += u64::from(self.detect_elapsed);
        EntryReport {
            elapsed: self.detect_elapsed,
            outcome: outcome((), self.detect_outcome),
        }
    }
}

fn outcome<T>(value: T, injection: Option<Injection>) -> EntryOutcome<T, &'static str> {
    match injection {
        Some(Injection::Host) => EntryOutcome::HostFailure("host"),
        Some(Injection::Released) => EntryOutcome::CriticalAbortReleased,
        Some(Injection::Unavailable) => EntryOutcome::TimingUnavailable,
        Some(Injection::ShortComplete) | None => EntryOutcome::Complete(value),
    }
}

struct DefaultDetection(Device);

impl EntryBus for DefaultDetection {
    type Error = &'static str;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.0.read_entry(access, start)
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.0.write_entry(access, value, start)
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.0.idle_at(start, clocks);
    }
}

fn capture() -> Group0Capture {
    Group0Capture {
        vector: 2,
        internal_pc: 0x7654_3210,
        phase_sr: TRACE | 0x051b,
        ird: 0xbad5,
        logical_address: 0x1234_5679,
        function_code: FunctionCode::UserProgram,
        direction: Direction::Read,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    }
}

fn imported(registers: Registers, capture: Group0Capture, clock: u64) -> PrefetchCpu {
    PrefetchCpu::from_group0_boundary(ImportedGroup0Boundary {
        registers,
        capture,
        clock: CpuClocks(clock),
    })
    .unwrap()
}

fn reset_boundary(clock: u64) -> ExternalResetBoundary {
    ExternalResetBoundary {
        sampled_release: CpuClocks(clock),
        qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
    }
}

fn registers() -> Registers {
    let mut registers = Registers::new();
    registers.d = std::array::from_fn(|i| 0xd012_0300 + i as u32);
    registers.a = std::array::from_fn(|i| 0xa045_0600 + i as u32);
    registers.usp = 0x4567_8000;
    registers.ssp = 0x89ab_c100;
    registers.pc = 0x3456_2000;
    registers.set_status_register(TRACE | 0x0315);
    registers
}

fn access(address: u32, function_code: FunctionCode, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_ffff,
        function_code,
        kind,
        lanes: ByteLanes::Both,
    }
}

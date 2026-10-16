use std::collections::BTreeMap;

use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryOutcome, EntryReport, ExternalResetBoundary,
    Group0Capture, PhysicalEntryState, PhysicalInspection, ProcessingClass, ResetQualification,
    SswProfile,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionPhase, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimingFault};
use zeff_m68k::{Registers, SUPERVISOR, TRACE};

#[path = "physical_instruction/boundary.rs"]
mod boundary;
#[path = "physical_instruction/capture.rs"]
mod capture;
#[path = "physical_instruction/completion.rs"]
mod completion;
#[path = "physical_instruction/terminal.rs"]
mod terminal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Instruction(BusAccess, u64),
    Detection(BusAccess, u64),
    EntryRead(BusAccess, u64),
    EntryWrite(BusAccess, u16, u64),
    Idle(u64, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Host,
    Boundary,
    Unavailable,
    ShortComplete,
}

struct Device {
    clock: u64,
    events: Vec<Event>,
    words: BTreeMap<u32, u16>,
    instruction_count: usize,
    entry_count: usize,
    instruction_elapsed: [u32; 2],
    instruction_failure: Option<(usize, Failure)>,
    detection: Option<(Failure, u32)>,
    entry_elapsed: [u32; 11],
    entry_failure: Option<(usize, Failure)>,
}

impl Device {
    fn new(clock: u64) -> Self {
        let mut device = Self {
            clock,
            events: Vec::new(),
            words: BTreeMap::new(),
            instruction_count: 0,
            entry_count: 0,
            instruction_elapsed: [4; 2],
            instruction_failure: None,
            detection: None,
            entry_elapsed: [4; 11],
            entry_failure: None,
        };
        device.word(8, 0x1234);
        device.word(10, 0x4000);
        device.word(12, 0x1234);
        device.word(14, 0x4000);
        device.word(0x1234_4000, 0x4e71);
        device.word(0x1234_4002, 0x7201);
        device
    }

    fn word(&mut self, logical: u32, value: u16) {
        self.words.insert(logical & BUS_ADDRESS_MASK, value);
    }

    fn start(&self, clock: CpuClocks) {
        assert_eq!(clock.0, self.clock, "device time ownership");
    }

    fn idle(&mut self, clock: CpuClocks, elapsed: u32) {
        self.start(clock);
        self.events.push(Event::Idle(clock.0, elapsed));
        self.clock += u64::from(elapsed);
    }

    fn entry_report<T>(&mut self, clock: CpuClocks, value: T) -> EntryReport<T, &'static str> {
        self.start(clock);
        let index = self.entry_count;
        self.entry_count += 1;
        let failure = self
            .entry_failure
            .filter(|(at, _)| *at == index)
            .map(|(_, failure)| failure);
        let elapsed = if failure == Some(Failure::ShortComplete) {
            3
        } else {
            self.entry_elapsed[index]
        };
        self.clock += u64::from(elapsed);
        EntryReport {
            elapsed,
            outcome: match failure {
                Some(Failure::Host) => EntryOutcome::HostFailure("host"),
                Some(Failure::Boundary) => EntryOutcome::CriticalAbortReleased,
                Some(Failure::Unavailable) => EntryOutcome::TimingUnavailable,
                Some(Failure::ShortComplete) | None => EntryOutcome::Complete(value),
            },
        }
    }
}

impl EntryBus for Device {
    type Error = &'static str;
    fn read_entry(&mut self, access: BusAccess, clock: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.events.push(Event::EntryRead(access, clock.0));
        let value = self
            .words
            .get(&access.bus_address)
            .copied()
            .unwrap_or(0x4e71);
        self.entry_report(clock, value)
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.events.push(Event::EntryWrite(access, value, clock.0));
        self.word(access.logical_address, value);
        self.entry_report(clock, ())
    }
    fn idle_entry(&mut self, clock: CpuClocks, elapsed: u32) {
        self.idle(clock, elapsed);
    }
}

impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        clock: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.start(clock);
        self.events.push(Event::Instruction(access, clock.0));
        let index = self.instruction_count;
        self.instruction_count += 1;
        let failure = self
            .instruction_failure
            .filter(|(at, _)| *at == index)
            .map(|(_, failure)| failure);
        let elapsed = if failure == Some(Failure::ShortComplete) {
            3
        } else {
            self.instruction_elapsed[index]
        };
        self.clock += u64::from(elapsed);
        let value = self
            .words
            .get(&access.bus_address)
            .copied()
            .unwrap_or(0x4e71);
        InstructionReport {
            elapsed,
            outcome: instruction_outcome(value, failure),
        }
    }

    fn write_instruction(
        &mut self,
        _: BusAccess,
        _: u16,
        _: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        panic!("unexpected instruction write")
    }

    fn detect_instruction_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        clock: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        assert_eq!(direction, Direction::Read);
        self.start(clock);
        match self.detection {
            Some((failure, elapsed)) => {
                self.events.push(Event::Detection(access, clock.0));
                self.clock += u64::from(elapsed);
                InstructionReport {
                    elapsed,
                    outcome: instruction_outcome((), Some(failure)),
                }
            }
            None => InstructionReport {
                elapsed: 0,
                outcome: InstructionOutcome::TimingUnavailable,
            },
        }
    }
}

fn instruction_outcome<T>(
    value: T,
    failure: Option<Failure>,
) -> InstructionOutcome<T, &'static str> {
    match failure {
        Some(Failure::Host) => InstructionOutcome::HostFailure("host"),
        Some(Failure::Boundary) => InstructionOutcome::AtGroup0Entry,
        Some(Failure::Unavailable) => InstructionOutcome::TimingUnavailable,
        Some(Failure::ShortComplete) | None => InstructionOutcome::Complete(value),
    }
}

impl ClockedBus for Device {
    type Error = &'static str;
    fn read_cycle(&mut self, access: BusAccess, clock: CpuClocks) -> CycleResult<u16, Self::Error> {
        self.start(clock);
        self.events.push(Event::Instruction(access, clock.0));
        let index = self.instruction_count;
        self.instruction_count += 1;
        let elapsed = self.instruction_elapsed.get(index).copied().unwrap_or(4);
        self.clock += u64::from(elapsed);
        CycleResult {
            result: if self.instruction_failure == Some((index, Failure::Host)) {
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
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), Self::Error> {
        panic!("unexpected legacy write")
    }
    fn idle(&mut self, clock: CpuClocks, elapsed: u32) {
        self.idle(clock, elapsed);
    }
}

struct DefaultInstructionDetection(Device);

impl EntryBus for DefaultInstructionDetection {
    type Error = &'static str;
    fn read_entry(&mut self, access: BusAccess, clock: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.0.read_entry(access, clock)
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.0.write_entry(access, value, clock)
    }
    fn idle_entry(&mut self, clock: CpuClocks, elapsed: u32) {
        self.0.idle(clock, elapsed);
    }
}

impl InstructionBus for DefaultInstructionDetection {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        clock: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.0.read_instruction(access, clock)
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.0.write_instruction(access, value, clock)
    }
}

fn registers(sr: u16, pc: u32) -> Registers {
    let mut registers = Registers::new();
    registers.d = std::array::from_fn(|i| 0xaabb_cc00 + i as u32);
    registers.a = std::array::from_fn(|i| 0x1122_3300 + i as u32);
    registers.usp = 0x0100_7000;
    registers.ssp = 0x8765_6000;
    registers.pc = pc;
    registers.set_status_register(sr);
    registers
}

fn access(address: u32, supervisor: bool) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & BUS_ADDRESS_MASK & !1,
        function_code: if supervisor {
            FunctionCode::SupervisorProgram
        } else {
            FunctionCode::UserProgram
        },
        kind: AccessKind::Prefetch,
        lanes: ByteLanes::Both,
    }
}

fn moveq(registers: &mut Registers, register: usize, immediate: u8) {
    let value = i32::from(immediate as i8) as u32;
    registers.d[register] = value;
    registers.set_status_register(
        (registers.status_register() & !15)
            | if value == 0 {
                4
            } else if immediate & 0x80 != 0 {
                8
            } else {
                0
            },
    );
}

fn assert_frame(device: &Device, sp: u32, captured: Group0Capture) {
    let ssw = (captured.ird & 0xffe0) | captured.function_code as u16 | 16;
    let bytes: Vec<_> = [
        ssw,
        (captured.logical_address >> 16) as u16,
        captured.logical_address as u16,
        captured.ird,
        captured.phase_sr,
        (captured.internal_pc >> 16) as u16,
        captured.internal_pc as u16,
    ]
    .into_iter()
    .flat_map(u16::to_be_bytes)
    .collect();
    let actual: Vec<_> = (0..7)
        .flat_map(|i| {
            device.words[&(sp.wrapping_sub(14).wrapping_add(i * 2) & BUS_ADDRESS_MASK)]
                .to_be_bytes()
        })
        .collect();
    assert_eq!(actual, bytes);
    let expected: Vec<_> = [
        (2, captured.internal_pc as u16),
        (6, captured.phase_sr),
        (4, (captured.internal_pc >> 16) as u16),
        (8, captured.ird),
        (10, captured.logical_address as u16),
        (14, ssw),
        (12, (captured.logical_address >> 16) as u16),
    ]
    .into_iter()
    .map(|(offset, value)| (sp.wrapping_sub(offset), value))
    .collect();
    let actual: Vec<_> = device
        .events
        .iter()
        .filter_map(|event| match event {
            Event::EntryWrite(access, value, _) => {
                assert_eq!(access.function_code, FunctionCode::SupervisorData);
                assert_eq!(access.kind, AccessKind::Stack);
                Some((access.logical_address, *value))
            }
            _ => None,
        })
        .collect();
    assert_eq!(actual, expected);
}

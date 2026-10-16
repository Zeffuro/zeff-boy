use std::collections::BTreeMap;

use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryOutcome, EntryReport, Group0Capture,
    PhysicalEntryState, PhysicalInspection, ProcessingClass, SswProfile,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionPhase, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimingFault};
use zeff_m68k::{Registers, SUPERVISOR, TRACE};

#[path = "physical_move/capture.rs"]
mod capture;
#[path = "physical_move/completion.rs"]
mod completion;
#[path = "physical_move/oracle.rs"]
mod oracle;
#[path = "physical_move/terminal.rs"]
mod terminal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Size {
    Byte,
    Word,
    Long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Form {
    size: Size,
    write: bool,
}

const FORMS: [Form; 6] = [
    Form {
        size: Size::Byte,
        write: false,
    },
    Form {
        size: Size::Word,
        write: false,
    },
    Form {
        size: Size::Long,
        write: false,
    },
    Form {
        size: Size::Byte,
        write: true,
    },
    Form {
        size: Size::Word,
        write: true,
    },
    Form {
        size: Size::Long,
        write: true,
    },
];

impl Form {
    fn opcode(self, dn: u16, an: u16) -> u16 {
        let base = match self.size {
            Size::Byte => 0x1000,
            Size::Word => 0x3000,
            Size::Long => 0x2000,
        };
        base | if self.write {
            0x0080 | an << 9 | dn
        } else {
            0x0010 | dn << 9 | an
        }
    }
    fn count(self) -> usize {
        if self.size == Size::Long { 3 } else { 2 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Host,
    Boundary,
    Unavailable,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Read(BusAccess, u64),
    Write(BusAccess, u16, u64),
    Detect(BusAccess, Direction, u64),
    EntryRead(BusAccess, u64),
    EntryWrite(BusAccess, u16, u64),
    Idle(u64, u32),
}

struct Device {
    clock: u64,
    events: Vec<Event>,
    words: BTreeMap<u32, u16>,
    count: usize,
    elapsed: [u32; 3],
    failure: Option<(usize, Failure)>,
    detection: Option<(Failure, u32)>,
    entry_count: usize,
    entry_failure: Option<(usize, Failure)>,
}

impl Device {
    fn new(clock: u64) -> Self {
        let mut device = Self {
            clock,
            events: Vec::new(),
            words: BTreeMap::new(),
            count: 0,
            elapsed: [4; 3],
            failure: None,
            detection: None,
            entry_count: 0,
            entry_failure: None,
        };
        for (address, value) in [
            (8, 0x4321),
            (10, 0x4000),
            (12, 0x4321),
            (14, 0x4000),
            (0x4321_4000, 0x4e71),
            (0x4321_4002, 0x7201),
        ] {
            device.word(address, value);
        }
        device
    }
    fn word(&mut self, address: u32, value: u16) {
        self.words.insert(address & BUS_ADDRESS_MASK & !1, value);
    }
    fn operand(&mut self, address: u32, size: Size, value: u32) {
        match size {
            Size::Byte => self.word(
                address,
                if address & 1 == 0 {
                    (value as u16 & 255) << 8 | 0x55
                } else {
                    0xaa00 | value as u16 & 255
                },
            ),
            Size::Word => self.word(address, value as u16),
            Size::Long => {
                self.word(address, (value >> 16) as u16);
                self.word(address.wrapping_add(2), value as u16);
            }
        }
    }
    fn start(&self, clock: CpuClocks) {
        assert_eq!(clock.0, self.clock, "device clock ownership");
    }
    fn idle_at(&mut self, clock: CpuClocks, clocks: u32) {
        self.start(clock);
        self.events.push(Event::Idle(clock.0, clocks));
        self.clock += u64::from(clocks);
    }
    fn apply(&mut self, access: BusAccess, value: u16) {
        let old = self.words.get(&access.bus_address).copied().unwrap_or(0);
        self.words.insert(
            access.bus_address,
            match access.lanes {
                ByteLanes::Both => value,
                ByteLanes::Upper => (old & 255) | (value & 0xff00),
                ByteLanes::Lower => (old & 0xff00) | (value & 255),
            },
        );
    }
    fn report<T>(&mut self, clock: CpuClocks, value: T) -> InstructionReport<T, &'static str> {
        self.start(clock);
        let index = self.count;
        self.count += 1;
        let failure = self
            .failure
            .filter(|(at, _)| *at == index)
            .map(|(_, failure)| failure);
        let elapsed = self.elapsed[index];
        self.clock += u64::from(elapsed);
        InstructionReport {
            elapsed,
            outcome: match failure {
                Some(Failure::Host) => InstructionOutcome::HostFailure("host"),
                Some(Failure::Boundary) => InstructionOutcome::AtGroup0Entry,
                Some(Failure::Unavailable) => InstructionOutcome::TimingUnavailable,
                Some(Failure::Complete) | None => InstructionOutcome::Complete(value),
            },
        }
    }
    fn entry_report<T>(&mut self, clock: CpuClocks, value: T) -> EntryReport<T, &'static str> {
        self.start(clock);
        let index = self.entry_count;
        self.entry_count += 1;
        self.clock += 4;
        let failure = self
            .entry_failure
            .filter(|(at, _)| *at == index)
            .map(|(_, failure)| failure);
        EntryReport {
            elapsed: 4,
            outcome: match failure {
                Some(Failure::Host) => EntryOutcome::HostFailure("host"),
                Some(Failure::Boundary) => EntryOutcome::CriticalAbortReleased,
                Some(Failure::Unavailable) => EntryOutcome::TimingUnavailable,
                Some(Failure::Complete) | None => EntryOutcome::Complete(value),
            },
        }
    }
}

impl EntryBus for Device {
    type Error = &'static str;
    fn read_entry(&mut self, access: BusAccess, clock: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.events.push(Event::EntryRead(access, clock.0));
        self.entry_report(
            clock,
            self.words
                .get(&access.bus_address)
                .copied()
                .unwrap_or(0x4e71),
        )
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.events.push(Event::EntryWrite(access, value, clock.0));
        self.apply(access, value);
        self.entry_report(clock, ())
    }
    fn idle_entry(&mut self, clock: CpuClocks, clocks: u32) {
        self.idle_at(clock, clocks);
    }
}

impl InstructionBus for Device {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        clock: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.events.push(Event::Read(access, clock.0));
        self.report(
            clock,
            self.words
                .get(&access.bus_address)
                .copied()
                .unwrap_or(0x4e71),
        )
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        clock: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.events.push(Event::Write(access, value, clock.0));
        self.apply(access, value);
        self.report(clock, ())
    }
    fn detect_instruction_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        clock: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.start(clock);
        match self.detection {
            Some((failure, elapsed)) => {
                self.events.push(Event::Detect(access, direction, clock.0));
                self.clock += u64::from(elapsed);
                InstructionReport {
                    elapsed,
                    outcome: match failure {
                        Failure::Host => InstructionOutcome::HostFailure("host"),
                        Failure::Boundary => InstructionOutcome::AtGroup0Entry,
                        Failure::Unavailable => InstructionOutcome::TimingUnavailable,
                        Failure::Complete => InstructionOutcome::Complete(()),
                    },
                }
            }
            None => InstructionReport {
                elapsed: 0,
                outcome: InstructionOutcome::TimingUnavailable,
            },
        }
    }
}

impl ClockedBus for Device {
    type Error = &'static str;
    fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, Self::Error> {
        panic!("unexpected legacy read")
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), Self::Error> {
        panic!("unexpected legacy write")
    }
    fn idle(&mut self, _: CpuClocks, _: u32) {
        panic!("unexpected legacy idle")
    }
}

fn registers(supervisor: bool, ccr: u16) -> Registers {
    let mut registers = Registers::new();
    registers.d = std::array::from_fn(|i| 0xaabb_cc00 | i as u32);
    registers.a = std::array::from_fn(|i| 0x1234_6000 + i as u32 * 16);
    registers.usp = 0x3456_7000;
    registers.ssp = 0x8765_8000;
    registers.pc = 0x2345_2000;
    registers.set_status_register(if supervisor { SUPERVISOR } else { 0 } | ccr);
    registers
}

fn seed(
    form: Form,
    mut initial: Registers,
    dn: u16,
    an: u16,
    value: u32,
    address: u32,
) -> (PrefetchCpu, Device, Registers) {
    initial.set_address_register(usize::from(an), address);
    if form.write {
        initial.d[usize::from(dn)] = value;
    }
    let mut bus = Device::new(37);
    bus.operand(
        address,
        form.size,
        if form.write { 0xdead_beef } else { value },
    );
    bus.word(initial.pc.wrapping_add(4), 0x7480);
    let cpu = PrefetchCpu::from_prefetched(initial, [form.opcode(dn, an), 0x7201], CpuClocks(37))
        .unwrap();
    (cpu, bus, initial)
}

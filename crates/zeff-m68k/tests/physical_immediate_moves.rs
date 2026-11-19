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

#[path = "physical_immediate_moves/capture.rs"]
mod capture;
#[path = "physical_immediate_moves/completion.rs"]
mod completion;
#[path = "physical_immediate_moves/detection.rs"]
mod detection;
#[path = "physical_move_extensions/device.rs"]
mod device;
#[path = "physical_immediate_moves/literals.rs"]
mod literals;
#[path = "physical_immediate_moves/oracle.rs"]
mod oracle;
#[path = "physical_immediate_moves/terminal.rs"]
mod terminal;
use device::{DefaultDetection, Device, Event, Failure};
type BusDevice = Device<7>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Size {
    Byte,
    Word,
    Long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Indirect,
    Post,
    Pre,
    Displacement,
    Indexed,
    AbsoluteWord,
    AbsoluteLong,
}

const FAMILIES: [Family; 7] = [
    Family::Indirect,
    Family::Post,
    Family::Pre,
    Family::Displacement,
    Family::Indexed,
    Family::AbsoluteWord,
    Family::AbsoluteLong,
];

#[derive(Clone, Copy, Debug)]
struct Form {
    size: Size,
    family: Family,
}

fn forms() -> Vec<Form> {
    [Size::Byte, Size::Word, Size::Long]
        .into_iter()
        .flat_map(|size| {
            FAMILIES
                .into_iter()
                .map(move |family| Form { size, family })
        })
        .collect()
}

impl Form {
    fn variable(self) -> bool {
        !matches!(self.family, Family::AbsoluteWord | Family::AbsoluteLong)
    }
    fn indexed(self) -> bool {
        self.family == Family::Indexed
    }
    fn prefix(self) -> usize {
        if self.size == Size::Long { 2 } else { 1 }
    }
    fn extensions(self) -> usize {
        match self.family {
            Family::Displacement | Family::Indexed | Family::AbsoluteWord => 1,
            Family::AbsoluteLong => 2,
            _ => 0,
        }
    }
    fn length(self) -> u32 {
        2 + 2 * (self.prefix() + self.extensions()) as u32
    }
    fn count(self) -> usize {
        self.prefix() + self.extensions() + 2 + usize::from(self.size == Size::Long)
    }
    fn stride(self, destination: u16) -> u32 {
        match self.size {
            Size::Byte if destination != 7 => 1,
            Size::Byte | Size::Word => 2,
            Size::Long => 4,
        }
    }
    fn opcode(self, destination: u16) -> u16 {
        let top = match self.size {
            Size::Byte => 0x1000,
            Size::Word => 0x3000,
            Size::Long => 0x2000,
        };
        let (mode, reg) = match self.family {
            Family::Indirect => (2, destination),
            Family::Post => (3, destination),
            Family::Pre => (4, destination),
            Family::Displacement => (5, destination),
            Family::Indexed => (6, destination),
            Family::AbsoluteWord => (7, 0),
            Family::AbsoluteLong => (7, 1),
        };
        top | reg << 9 | mode << 6 | 0x3c
    }
}

#[derive(Clone, Copy)]
struct Case {
    form: Form,
    r: Registers,
    destination: u16,
    value: u32,
    low: u16,
    extension: u16,
    destination_low: u16,
    next: u16,
    last: u16,
    start: u64,
}

impl Case {
    fn new(form: Form) -> Self {
        Self {
            form,
            r: registers(false, 31),
            destination: 3,
            value: 0x8000_0001,
            low: 1,
            extension: match form.family {
                Family::Displacement => 0xfff0,
                Family::Indexed => 0x48f0,
                Family::AbsoluteWord => 0x9000,
                _ => 0xabcd,
            },
            destination_low: 0x6000,
            next: 0x7201,
            last: 0x7480,
            start: 37,
        }
    }
    fn anchor(self) -> u32 {
        self.r.pc.wrapping_add(2 + 2 * self.form.prefix() as u32)
    }
    fn e0(self) -> u16 {
        if self.form.size == Size::Long {
            (self.value >> 16) as u16
        } else {
            self.value as u16
        }
    }
    fn effective(self, extension: u16, low: u16) -> u32 {
        let base = self
            .r
            .address_register(usize::from(self.destination))
            .unwrap();
        match self.form.family {
            Family::Indirect | Family::Post => base,
            Family::Pre => base.wrapping_sub(self.form.stride(self.destination)),
            Family::Displacement => base.wrapping_add_signed(i32::from(extension as i16)),
            Family::Indexed => {
                let index = usize::from((extension >> 12) & 7);
                let raw = if extension & 0x8000 != 0 {
                    self.r.address_register(index).unwrap()
                } else {
                    self.r.d[index]
                };
                let value = if extension & 0x800 != 0 {
                    raw
                } else {
                    i32::from(raw as u16 as i16) as u32
                };
                base.wrapping_add(value)
                    .wrapping_add_signed(i32::from(extension as u8 as i8))
            }
            Family::AbsoluteWord => i32::from(extension as i16) as u32,
            Family::AbsoluteLong => u32::from(extension) << 16 | u32::from(low),
        }
    }
    fn seed(self) -> (PrefetchCpu, BusDevice, Self) {
        let mut bus = BusDevice::with_clock(self.start);
        bus.operand(
            self.effective(self.extension, self.destination_low),
            self.form.size,
            0xdead_beef,
        );
        bus.word(self.r.pc.wrapping_add(2), 0x1337);
        if self.form.size == Size::Long {
            bus.word(self.r.pc.wrapping_add(4), self.low);
        }
        if self.form.extensions() != 0 {
            bus.word(self.anchor(), self.extension);
        }
        if self.form.extensions() == 2 {
            bus.word(self.anchor().wrapping_add(2), self.destination_low);
        }
        bus.word(self.r.pc.wrapping_add(self.form.length()), self.next);
        bus.word(self.r.pc.wrapping_add(self.form.length() + 2), self.last);
        let cpu = PrefetchCpu::from_prefetched(
            self.r,
            [self.form.opcode(self.destination), self.e0()],
            CpuClocks(self.start),
        )
        .unwrap();
        (cpu, bus, self)
    }
}

fn registers(supervisor: bool, ccr: u16) -> Registers {
    let mut r = Registers::new();
    r.d = std::array::from_fn(|i| 0xaabb_cc00 | (i as u32 * 2));
    r.a = std::array::from_fn(|i| 0x1234_6000 + i as u32 * 16);
    r.usp = 0x3456_7000;
    r.ssp = 0x8765_8000;
    r.pc = 0x2345_2000;
    r.set_status_register(if supervisor { SUPERVISOR } else { 0 } | ccr);
    r
}

fn guarded(cpu: &mut PrefetchCpu, bus: &mut BusDevice, fault: TimingFault<&'static str>) {
    let before = cpu.clone();
    let events = bus.events.clone();
    let memory = bus.words.clone();
    let clock = bus.clock;
    assert_eq!(cpu.prime(bus), Err(fault.clone()));
    assert_eq!(cpu.step_instruction(bus), Err(fault.clone()));
    assert_eq!(cpu.step_boundary(bus), Err(fault.clone()));
    assert_eq!(cpu.service_pending_trace(bus), Err(fault.clone()));
    assert_eq!(
        cpu.step_physical_instruction(bus),
        Err(InstructionFault::Execution(fault))
    );
    assert_eq!(*cpu, before);
    assert_eq!(bus.events, events);
    assert_eq!(bus.words, memory);
    assert_eq!(bus.clock, clock);
}

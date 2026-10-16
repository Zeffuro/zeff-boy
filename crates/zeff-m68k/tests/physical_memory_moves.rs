use std::collections::BTreeMap;

use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryOutcome, EntryReport, Group0Capture,
    PhysicalEntryState, PhysicalInspection, ProcessingClass, SswProfile, TimingQualification,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionPhase, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimingFault};
use zeff_m68k::{Registers, SUPERVISOR, TRACE};

#[path = "physical_memory_moves/capture.rs"]
mod capture;
#[path = "physical_memory_moves/completion.rs"]
mod completion;
#[path = "physical_memory_moves/detection.rs"]
mod detection;
#[path = "physical_move_extensions/device.rs"]
mod device;
#[path = "physical_memory_moves/literals.rs"]
mod literals;
#[path = "physical_memory_moves/oracle.rs"]
mod oracle;
#[path = "physical_memory_moves/terminal.rs"]
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
enum Source {
    Indirect,
    Post,
    Pre,
    Displacement,
    Indexed,
    AbsoluteWord,
    AbsoluteLong,
    PcDisplacement,
    PcIndexed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destination {
    Indirect,
    Post,
    Pre,
}

const SOURCES: [Source; 9] = [
    Source::Indirect,
    Source::Post,
    Source::Pre,
    Source::Displacement,
    Source::Indexed,
    Source::AbsoluteWord,
    Source::AbsoluteLong,
    Source::PcDisplacement,
    Source::PcIndexed,
];
const DESTINATIONS: [Destination; 3] = [Destination::Indirect, Destination::Post, Destination::Pre];

#[derive(Clone, Copy, Debug)]
struct Form {
    size: Size,
    source: Source,
    destination: Destination,
}

fn forms() -> Vec<Form> {
    [Size::Byte, Size::Word, Size::Long]
        .into_iter()
        .flat_map(|size| {
            SOURCES.into_iter().flat_map(move |source| {
                DESTINATIONS.into_iter().map(move |destination| Form {
                    size,
                    source,
                    destination,
                })
            })
        })
        .collect()
}

impl Form {
    fn variable(self) -> bool {
        matches!(
            self.source,
            Source::Indirect | Source::Post | Source::Pre | Source::Displacement | Source::Indexed
        )
    }
    fn idle(self) -> bool {
        matches!(
            self.source,
            Source::Pre | Source::Indexed | Source::PcIndexed
        )
    }
    fn extensions(self) -> usize {
        match self.source {
            Source::Indirect | Source::Post | Source::Pre => 0,
            Source::AbsoluteLong => 2,
            _ => 1,
        }
    }
    fn length(self) -> u32 {
        2 + 2 * self.extensions() as u32
    }
    fn reads(self) -> usize {
        self.extensions() + 1 + usize::from(self.size == Size::Long)
    }
    fn count(self) -> usize {
        self.reads() + 2 + usize::from(self.size == Size::Long)
    }
    fn stride(self, register: u16) -> u32 {
        match self.size {
            Size::Byte if register != 7 => 1,
            Size::Byte | Size::Word => 2,
            Size::Long => 4,
        }
    }
    fn opcode(self, source: u16, destination: u16) -> u16 {
        let top = match self.size {
            Size::Byte => 0x1000,
            Size::Word => 0x3000,
            Size::Long => 0x2000,
        };
        let field = match self.source {
            Source::Indirect => 0x10 | source,
            Source::Post => 0x18 | source,
            Source::Pre => 0x20 | source,
            Source::Displacement => 0x28 | source,
            Source::Indexed => 0x30 | source,
            Source::AbsoluteWord => 0x38,
            Source::AbsoluteLong => 0x39,
            Source::PcDisplacement => 0x3a,
            Source::PcIndexed => 0x3b,
        };
        let mode = match self.destination {
            Destination::Indirect => 2,
            Destination::Post => 3,
            Destination::Pre => 4,
        };
        top | destination << 9 | mode << 6 | field
    }
}

#[derive(Clone, Copy)]
struct Case {
    form: Form,
    r: Registers,
    source: u16,
    destination: u16,
    value: u32,
    extension: u16,
    source_low: u16,
    next: u16,
    last: u16,
    start: u64,
}

impl Case {
    fn new(form: Form) -> Self {
        Self {
            form,
            r: registers(false, 31),
            source: 3,
            destination: 6,
            value: 0x8000_0001,
            extension: match form.source {
                Source::Displacement | Source::PcDisplacement => 0xfff0,
                Source::Indexed | Source::PcIndexed => 0x40f0,
                Source::AbsoluteWord => 0x9000,
                Source::AbsoluteLong => 0xabcd,
                _ => 0,
            },
            source_low: 0x6000,
            next: 0x7201,
            last: 0x7480,
            start: 37,
        }
    }
    fn e0(self) -> u16 {
        if self.form.extensions() == 0 {
            self.next
        } else {
            self.extension
        }
    }
    fn source_address(self, low: u16) -> u32 {
        let base = if matches!(self.form.source, Source::PcDisplacement | Source::PcIndexed) {
            self.r.pc.wrapping_add(2)
        } else {
            self.r.address_register(usize::from(self.source)).unwrap()
        };
        match self.form.source {
            Source::Indirect | Source::Post => base,
            Source::Pre => base.wrapping_sub(self.form.stride(self.source)),
            Source::Displacement | Source::PcDisplacement => {
                base.wrapping_add_signed(i32::from(self.extension as i16))
            }
            Source::Indexed | Source::PcIndexed => {
                let index = usize::from(self.extension >> 12 & 7);
                let raw = if self.extension & 0x8000 == 0 {
                    self.r.d[index]
                } else {
                    self.r.address_register(index).unwrap()
                };
                let value = if self.extension & 0x800 == 0 {
                    i32::from(raw as i16) as u32
                } else {
                    raw
                };
                base.wrapping_add(value)
                    .wrapping_add_signed(i32::from(self.extension as u8 as i8))
            }
            Source::AbsoluteWord => i32::from(self.extension as i16) as u32,
            Source::AbsoluteLong => u32::from(self.extension) << 16 | u32::from(low),
        }
    }
    fn after_source(self) -> Registers {
        let mut r = self.r;
        if matches!(self.form.source, Source::Post | Source::Pre) {
            let base = self.r.address_register(usize::from(self.source)).unwrap();
            r.set_address_register(
                usize::from(self.source),
                if self.form.source == Source::Post {
                    base.wrapping_add(self.form.stride(self.source))
                } else {
                    base.wrapping_sub(self.form.stride(self.source))
                },
            );
        }
        r
    }
    fn destination_address(self) -> u32 {
        let base = self
            .after_source()
            .address_register(usize::from(self.destination))
            .unwrap();
        if self.form.destination == Destination::Pre {
            base.wrapping_sub(self.form.stride(self.destination))
        } else {
            base
        }
    }
    fn seed(self) -> (PrefetchCpu, BusDevice, Self) {
        let mut bus = BusDevice::with_clock(self.start);
        bus.operand(self.destination_address(), self.form.size, 0xdead_beef);
        bus.word(self.r.pc.wrapping_add(2), 0x1337);
        if self.form.extensions() == 2 {
            bus.word(self.r.pc.wrapping_add(4), self.source_low);
        }
        bus.word(self.r.pc.wrapping_add(self.form.length()), self.next);
        bus.word(self.r.pc.wrapping_add(self.form.length() + 2), self.last);
        bus.operand(
            self.source_address(self.source_low),
            self.form.size,
            self.value,
        );
        let cpu = PrefetchCpu::from_prefetched(
            self.r,
            [self.form.opcode(self.source, self.destination), self.e0()],
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

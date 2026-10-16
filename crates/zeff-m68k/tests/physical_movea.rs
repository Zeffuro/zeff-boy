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

#[path = "physical_movea/capture.rs"]
mod capture;
#[path = "physical_movea/completion.rs"]
mod completion;
#[path = "physical_movea/detection.rs"]
mod detection;
#[path = "physical_move_extensions/device.rs"]
mod device;
#[path = "physical_movea/literals.rs"]
mod literals;
#[path = "physical_movea/oracle.rs"]
mod oracle;
#[path = "physical_movea/terminal.rs"]
mod terminal;
use device::{DefaultDetection, Device, Event, Failure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Size {
    Byte,
    Word,
    Long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Data,
    Address,
    Indirect,
    Post,
    Pre,
    Displacement,
    Indexed,
    AbsoluteWord,
    AbsoluteLong,
    PcDisplacement,
    PcIndexed,
    Immediate,
}

const FAMILIES: [Family; 12] = [
    Family::Data,
    Family::Address,
    Family::Indirect,
    Family::Post,
    Family::Pre,
    Family::Displacement,
    Family::Indexed,
    Family::AbsoluteWord,
    Family::AbsoluteLong,
    Family::PcDisplacement,
    Family::PcIndexed,
    Family::Immediate,
];

#[derive(Clone, Copy, Debug)]
struct Form {
    size: Size,
    family: Family,
}

fn forms() -> Vec<Form> {
    [Size::Word, Size::Long]
        .into_iter()
        .flat_map(|size| {
            FAMILIES
                .into_iter()
                .map(move |family| Form { size, family })
        })
        .collect()
}

impl Form {
    fn registers(self) -> bool {
        matches!(self.family, Family::Data | Family::Address)
    }
    fn memory(self) -> bool {
        !self.registers() && self.family != Family::Immediate
    }
    fn variable(self) -> bool {
        matches!(
            self.family,
            Family::Data
                | Family::Address
                | Family::Indirect
                | Family::Post
                | Family::Pre
                | Family::Displacement
                | Family::Indexed
        )
    }
    fn idle(self) -> u64 {
        if matches!(
            self.family,
            Family::Pre | Family::Indexed | Family::PcIndexed
        ) {
            2
        } else {
            0
        }
    }
    fn length(self) -> u32 {
        match self.family {
            Family::Data | Family::Address | Family::Indirect | Family::Post | Family::Pre => 2,
            Family::AbsoluteLong | Family::Immediate if self.size == Size::Long => 6,
            Family::AbsoluteLong => 6,
            _ => 4,
        }
    }
    fn count(self) -> usize {
        ((self.length() - 2) / 2) as usize
            + 1
            + if !self.memory() {
                0
            } else if self.size == Size::Long {
                2
            } else {
                1
            }
    }
    fn opcode(self, destination: u16, source: u16) -> u16 {
        let top = match self.size {
            Size::Byte => 0x1000,
            Size::Word => 0x3000,
            Size::Long => 0x2000,
        };
        let (mode, reg) = match self.family {
            Family::Data => (0, source),
            Family::Address => (1, source),
            Family::Indirect => (2, source),
            Family::Post => (3, source),
            Family::Pre => (4, source),
            Family::Displacement => (5, source),
            Family::Indexed => (6, source),
            Family::AbsoluteWord => (7, 0),
            Family::AbsoluteLong => (7, 1),
            Family::PcDisplacement => (7, 2),
            Family::PcIndexed => (7, 3),
            Family::Immediate => (7, 4),
        };
        top | destination << 9 | 0x40 | mode << 3 | reg
    }
}

#[derive(Clone, Copy)]
struct Case {
    form: Form,
    r: Registers,
    destination: u16,
    source: u16,
    e0: u16,
    e4: u16,
    value: u32,
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
            source: 2,
            e0: if form.length() == 2 {
                0x7201
            } else {
                match form.family {
                    Family::Displacement | Family::PcDisplacement => 0xfff0,
                    Family::Indexed | Family::PcIndexed => 0xc8f0,
                    Family::AbsoluteWord => 0x6000,
                    _ => 0xabcd,
                }
            },
            e4: 0x6000,
            value: 0x8000_0000,
            next: 0x7201,
            last: 0x7480,
            start: 37,
        }
    }
    fn effective(self, low: u16) -> u32 {
        let base = if matches!(self.form.family, Family::PcDisplacement | Family::PcIndexed) {
            self.r.pc.wrapping_add(2)
        } else {
            self.r.address_register(usize::from(self.source)).unwrap()
        };
        match self.form.family {
            Family::Indirect | Family::Post => base,
            Family::Pre => base.wrapping_sub(if self.form.size == Size::Long { 4 } else { 2 }),
            Family::Displacement | Family::PcDisplacement => {
                base.wrapping_add_signed(i32::from(self.e0 as i16))
            }
            Family::Indexed | Family::PcIndexed => {
                let reg = usize::from((self.e0 >> 12) & 7);
                let raw = if self.e0 & 0x8000 != 0 {
                    self.r.address_register(reg).unwrap()
                } else {
                    self.r.d[reg]
                };
                let index = if self.e0 & 0x800 != 0 {
                    raw
                } else {
                    i32::from(raw as u16 as i16) as u32
                };
                base.wrapping_add(index)
                    .wrapping_add_signed(i32::from(self.e0 as u8 as i8))
            }
            Family::AbsoluteWord => i32::from(self.e0 as i16) as u32,
            Family::AbsoluteLong => u32::from(self.e0) << 16 | u32::from(low),
            _ => 0,
        }
    }
    fn seed(mut self) -> (PrefetchCpu, Device, Self) {
        let source_value = if self.form.size == Size::Long {
            self.value
        } else {
            0x1234_0000 | (self.value & 65535)
        };
        match self.form.family {
            Family::Data => self.r.d[usize::from(self.source)] = source_value,
            Family::Address => {
                assert!(
                    self.r
                        .set_address_register(usize::from(self.source), source_value)
                );
            }
            Family::Immediate => {
                self.e0 = if self.form.size == Size::Long {
                    (self.value >> 16) as u16
                } else {
                    self.value as u16
                };
                self.e4 = self.value as u16;
            }
            _ => {}
        }
        let mut bus = Device::new(self.start);
        if self.form.memory() {
            bus.operand(self.effective(self.e4), self.form.size, self.value);
        }
        bus.word(self.r.pc.wrapping_add(2), 0x1337);
        if self.form.length() == 6 {
            bus.word(self.r.pc.wrapping_add(4), self.e4);
        }
        if self.form.length() != 2 {
            bus.word(self.r.pc.wrapping_add(self.form.length()), self.next);
        }
        bus.word(self.r.pc.wrapping_add(self.form.length() + 2), self.last);
        let cpu = PrefetchCpu::from_prefetched(
            self.r,
            [self.form.opcode(self.destination, self.source), self.e0],
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

fn guarded(cpu: &mut PrefetchCpu, bus: &mut Device, fault: TimingFault<&'static str>) {
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

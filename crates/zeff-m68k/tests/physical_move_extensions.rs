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

#[path = "physical_move_extensions/capture.rs"]
mod capture;
#[path = "physical_move_extensions/completion.rs"]
mod completion;
#[path = "physical_move_extensions/detection.rs"]
mod detection;
#[path = "physical_move_extensions/device.rs"]
mod device;
#[path = "physical_move_extensions/literals.rs"]
mod literals;
#[path = "physical_move_extensions/oracle.rs"]
mod oracle;
#[path = "physical_move_extensions/terminal.rs"]
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
    Displacement,
    Indexed,
    AbsoluteWord,
    AbsoluteLong,
    PcDisplacement,
    PcIndexed,
    Immediate,
}

const FAMILIES: [Family; 7] = [
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
    write: bool,
}

fn forms() -> Vec<Form> {
    [Size::Byte, Size::Word, Size::Long]
        .into_iter()
        .flat_map(|size| {
            FAMILIES.into_iter().flat_map(move |family| {
                [false, true].into_iter().filter_map(move |write| {
                    (!write
                        || matches!(
                            family,
                            Family::Displacement
                                | Family::Indexed
                                | Family::AbsoluteWord
                                | Family::AbsoluteLong
                        ))
                    .then_some(Form {
                        size,
                        family,
                        write,
                    })
                })
            })
        })
        .collect()
}

impl Form {
    fn indexed(self) -> bool {
        matches!(self.family, Family::Indexed | Family::PcIndexed)
    }
    fn uses_an(self) -> bool {
        matches!(self.family, Family::Displacement | Family::Indexed)
    }
    fn memory(self) -> bool {
        self.family != Family::Immediate
    }
    fn length(self) -> u32 {
        if self.family == Family::AbsoluteLong
            || self.family == Family::Immediate && self.size == Size::Long
        {
            6
        } else {
            4
        }
    }
    fn count(self) -> usize {
        usize::from(self.length() == 6)
            + 2
            + if !self.memory() {
                0
            } else if self.size == Size::Long {
                2
            } else {
                1
            }
    }
    fn opcode(self, dn: u16, an: u16) -> u16 {
        let top = match self.size {
            Size::Byte => 0x1000,
            Size::Word => 0x3000,
            Size::Long => 0x2000,
        };
        let (mode, reg) = match self.family {
            Family::Displacement => (5, an),
            Family::Indexed => (6, an),
            Family::AbsoluteWord => (7, 0),
            Family::AbsoluteLong => (7, 1),
            Family::PcDisplacement => (7, 2),
            Family::PcIndexed => (7, 3),
            Family::Immediate => (7, 4),
        };
        top | if self.write {
            reg << 9 | mode << 6 | dn
        } else {
            dn << 9 | mode << 3 | reg
        }
    }
}

#[derive(Clone, Copy)]
struct Case {
    form: Form,
    r: Registers,
    dn: u16,
    an: u16,
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
            dn: 3,
            an: 2,
            e0: match form.family {
                Family::Displacement | Family::PcDisplacement => 0xfff0,
                Family::Indexed | Family::PcIndexed => 0xc8f0,
                Family::AbsoluteWord => 0x6000,
                Family::AbsoluteLong => 0xabcd,
                Family::Immediate => 0x8001,
            },
            e4: 0x6000,
            value: 0x8000_0001,
            next: 0x7201,
            last: 0x7480,
            start: 37,
        }
    }
    fn effective(self, low: u16) -> u32 {
        let base = if self.form.uses_an() {
            self.r.address_register(usize::from(self.an)).unwrap()
        } else {
            self.r.pc.wrapping_add(2)
        };
        match self.form.family {
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
                let index = if self.e0 & 0x0800 != 0 {
                    raw
                } else {
                    i32::from(raw as u16 as i16) as u32
                };
                base.wrapping_add(index)
                    .wrapping_add_signed(i32::from(self.e0 as u8 as i8))
            }
            Family::AbsoluteWord => i32::from(self.e0 as i16) as u32,
            Family::AbsoluteLong => (u32::from(self.e0) << 16) | u32::from(low),
            Family::Immediate => 0,
        }
    }
    fn seed(mut self) -> (PrefetchCpu, Device, Self) {
        if self.form.write {
            self.r.d[usize::from(self.dn)] = self.value;
        }
        let mut bus = Device::new(self.start);
        if self.form.memory() {
            bus.operand(
                self.effective(self.e4),
                self.form.size,
                if self.form.write {
                    0xdead_beef
                } else {
                    self.value
                },
            );
        }
        bus.word(self.r.pc.wrapping_add(2), 0x1337);
        if self.form.length() == 6 {
            bus.word(self.r.pc.wrapping_add(4), self.e4);
        }
        bus.word(self.r.pc.wrapping_add(self.form.length()), self.next);
        bus.word(self.r.pc.wrapping_add(self.form.length() + 2), self.last);
        let cpu = PrefetchCpu::from_prefetched(
            self.r,
            [self.form.opcode(self.dn, self.an), self.e0],
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

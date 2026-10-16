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

#[path = "physical_cmpa/capture.rs"]
mod capture;
#[path = "physical_cmpa/classifier.rs"]
mod classifier;
#[path = "physical_cmpa/completion.rs"]
mod completion;
#[path = "physical_cmpa/detection.rs"]
mod detection;
#[path = "physical_move_extensions/device.rs"]
mod device;
#[path = "physical_cmpa/directed.rs"]
mod directed;
#[path = "physical_cmpa/entry.rs"]
mod entry;
#[path = "physical_cmpa/literals.rs"]
mod literals;
#[path = "physical_cmpa/migration.rs"]
mod migration;
#[path = "physical_cmpa/oracle.rs"]
mod oracle;
#[path = "physical_cmpa/terminal.rs"]
mod terminal;
use device::{DefaultDetection, Device, Event, Failure};
type BusDevice = Device<5>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Size {
    Byte,
    Word,
    Long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ea {
    Indirect,
    Post,
    Pre,
    Displacement,
    Index,
    AbsoluteWord,
    AbsoluteLong,
    PcDisplacement,
    PcIndex,
}

const EAS: [Ea; 9] = [
    Ea::Indirect,
    Ea::Post,
    Ea::Pre,
    Ea::Displacement,
    Ea::Index,
    Ea::AbsoluteWord,
    Ea::AbsoluteLong,
    Ea::PcDisplacement,
    Ea::PcIndex,
];

#[derive(Clone, Copy, Debug)]
struct Form {
    size: Size,
    ea: Ea,
}

fn forms() -> Vec<Form> {
    [Size::Word, Size::Long]
        .into_iter()
        .flat_map(|size| EAS.into_iter().map(move |ea| Form { size, ea }))
        .collect()
}

impl Form {
    fn variable(self) -> bool {
        !matches!(
            self.ea,
            Ea::AbsoluteWord | Ea::AbsoluteLong | Ea::PcDisplacement | Ea::PcIndex
        )
    }
    fn extensions(self) -> usize {
        match self.ea {
            Ea::Indirect | Ea::Post | Ea::Pre => 0,
            Ea::AbsoluteLong => 2,
            _ => 1,
        }
    }
    fn idle(self) -> bool {
        matches!(self.ea, Ea::Pre | Ea::Index | Ea::PcIndex)
    }
    fn count(self) -> usize {
        self.extensions() + 2 + usize::from(self.size == Size::Long)
    }
    fn length(self) -> u32 {
        2 + 2 * self.extensions() as u32
    }
    fn stride(self) -> u32 {
        if self.size == Size::Long { 4 } else { 2 }
    }
    fn opcode(self, register: u16) -> u16 {
        let field = match self.ea {
            Ea::Indirect => 0x10 | register,
            Ea::Post => 0x18 | register,
            Ea::Pre => 0x20 | register,
            Ea::Displacement => 0x28 | register,
            Ea::Index => 0x30 | register,
            Ea::AbsoluteWord => 0x38,
            Ea::AbsoluteLong => 0x39,
            Ea::PcDisplacement => 0x3a,
            Ea::PcIndex => 0x3b,
        };
        0xb000
            | match self.size {
                Size::Byte => 0,
                Size::Word => 0xc0,
                Size::Long => 0x1c0,
            }
            | field
    }
}

#[derive(Clone, Copy)]
struct Case {
    form: Form,
    r: Registers,
    register: u16,
    destination: u16,
    expected_destination: Option<u32>,
    value: u32,
    extension: u16,
    low: u16,
    next: u16,
    last: u16,
    start: u64,
}

impl Case {
    fn new(form: Form) -> Self {
        Self {
            form,
            r: registers(false, 31),
            register: 3,
            destination: 5,
            expected_destination: None,
            value: 0x8000_0001,
            extension: match form.ea {
                Ea::Displacement | Ea::PcDisplacement => 0xfff0,
                Ea::Index | Ea::PcIndex => 0xe0f0,
                Ea::AbsoluteWord => 0x9000,
                Ea::AbsoluteLong => 0xabcd,
                _ => 0,
            },
            low: 0x6000,
            next: 0x7201,
            last: 0x7480,
            start: 37,
        }
    }
    fn opcode(self) -> u16 {
        self.form.opcode(self.register) | self.destination << 9
    }
    fn retained(self) -> u16 {
        if self.form.extensions() == 0 {
            self.next
        } else {
            self.extension
        }
    }
    fn seed(self) -> (PrefetchCpu, BusDevice) {
        let mut bus = Device::new(self.start);
        bus.word(self.r.pc, self.opcode());
        bus.word(self.r.pc.wrapping_add(2), self.retained());
        if self.form.extensions() == 2 {
            bus.word(self.r.pc.wrapping_add(4), self.low);
        }
        bus.word(self.r.pc.wrapping_add(self.form.length()), self.next);
        bus.word(self.r.pc.wrapping_add(self.form.length() + 2), self.last);
        let address = oracle::address(self, &bus.words);
        bus.operand(address, self.form.size, self.value);
        let cpu = PrefetchCpu::from_prefetched(
            self.r,
            [self.opcode(), self.retained()],
            CpuClocks(self.start),
        )
        .unwrap();
        (cpu, bus)
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

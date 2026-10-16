#[path = "../multi_load/support.rs"]
#[allow(dead_code)]
mod support;

use support::{Access, Bus};
use zeff_z80::{
    BusState, Cpu, CpuState, CpuTrap, InterruptMode, PrefixState, Registers, ResetState, Z80Bus,
};

#[path = "lifecycle.rs"]
mod lifecycle;
pub use lifecycle::{
    interrupt_pending_clone_suspend_reset, interrupt_priority_iff_and_ei_boundaries,
    interrupt_reset_halt_and_im0_guards,
};

#[derive(Clone, Copy, Debug)]
enum Service {
    Nmi,
    Im1,
    Im2,
    Rst(u8, u16),
}

const SERVICES: [Service; 11] = [
    Service::Nmi,
    Service::Im1,
    Service::Im2,
    Service::Rst(0xc7, 0),
    Service::Rst(0xcf, 8),
    Service::Rst(0xd7, 0x10),
    Service::Rst(0xdf, 0x18),
    Service::Rst(0xe7, 0x20),
    Service::Rst(0xef, 0x28),
    Service::Rst(0xf7, 0x30),
    Service::Rst(0xff, 0x38),
];

impl Service {
    fn setup(self) -> (u8, u8) {
        match self {
            Self::Nmi | Self::Im1 => (0x56, 0xaa),
            Self::Im2 => (0x5e, 0xff),
            Self::Rst(byte, _) => (0x46, byte),
        }
    }

    fn clocks(self) -> &'static [u32] {
        match self {
            Self::Nmi => &[5, 3, 3],
            Self::Im2 => &[7, 3, 3, 3, 3],
            _ => &[7, 3, 3],
        }
    }

    fn target(self) -> u16 {
        match self {
            Self::Nmi => 0x66,
            Self::Im1 => 0x38,
            Self::Im2 => 0x5678,
            Self::Rst(_, target) => target,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IrqBus {
    memory: Bus,
    byte: u8,
    irq_acks: u32,
    nmi_result: bool,
}

impl Z80Bus for IrqBus {
    fn cpu_read(&self, address: u16) -> u8 {
        self.memory.cpu_read(address)
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        self.memory.cpu_write(address, value);
    }
    fn io_read(&mut self, port: u8) -> u8 {
        self.memory.io_read(port)
    }
    fn io_write(&mut self, port: u8, value: u8) {
        self.memory.io_write(port, value);
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.memory.maskable_interrupt_pending()
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        self.memory.non_maskable_interrupt_pending()
    }
    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        self.irq_acks += 1;
        self.byte
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.memory.acknowledge_non_maskable_interrupt();
        self.nmi_result
    }
}

fn fixture(mode: u8, byte: u8, pc: u16, sp: u16, r: u8, i: u8) -> (Cpu, IrqBus) {
    let (mut cpu, mut memory) = support::seeded(0x12);
    let mut setup = vec![
        0x31,
        sp as u8,
        (sp >> 8) as u8,
        0xed,
        mode,
        0x3e,
        i,
        0xed,
        0x47,
    ];
    setup.extend_from_slice(&[0x3e, (r & 0x80) | (r.wrapping_sub(1) & 0x7f), 0xed, 0x4f]);
    setup.extend_from_slice(&[0xc3, pc as u8, (pc >> 8) as u8]);
    let start = usize::from(cpu.regs().pc);
    memory.bytes[start..start + setup.len()].copy_from_slice(&setup);
    for _ in 0..7 {
        cpu.step_with_bus(&mut memory).unwrap();
        memory.clear_trace();
    }
    assert_eq!(
        (cpu.regs().pc, cpu.regs().sp, cpu.regs().r, cpu.regs().i),
        (pc, sp, r, i)
    );
    cpu.finish_guest_call(true, true, 0);
    cpu.resume();
    memory.irq = true;
    memory.polls.set(0);
    (
        cpu,
        IrqBus {
            memory,
            byte,
            irq_acks: 0,
            nmi_result: true,
        },
    )
}

fn phase(cpu: &mut Cpu, bus: &mut IrqBus, state: &mut BusState, clocks: u32, complete: bool) {
    let step = cpu.step_bus_segment(bus, state).unwrap();
    assert_eq!(
        (step.instruction.cycles, step.complete, state.pending()),
        (clocks, complete, !complete)
    );
    if cpu.last_step_was_interrupt() {
        assert_eq!(
            (step.instruction.pc, step.instruction.opcode),
            (cpu.last_opcode_pc(), 0xff)
        );
    }
}

fn metadata(cpu: &Cpu, pc: u16) {
    assert_eq!(
        (
            cpu.last_opcode_pc(),
            cpu.last_opcode(),
            cpu.last_m1_fetch_count()
        ),
        (pc, 0xff, 1)
    );
    assert!(cpu.last_step_was_interrupt());
    assert!(cpu.instruction_bytes().is_empty());
}

fn first_acceptance() {
    let (mut cpu, mut bus) = fixture(0x56, 0xaa, 0x4000, 0x8000, 0xff, 0x12);
    let initial = cpu.regs();
    let cycles = cpu.cycles();
    let mut expected = initial;
    let mut state = BusState::default();
    phase(&mut cpu, &mut bus, &mut state, 7, false);
    expected.sp = 0x7fff;
    expected.r = 0x80;
    assert_eq!(cpu.regs(), expected);
    assert_eq!(
        (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
        (false, false)
    );
    assert_eq!(bus.irq_acks, 1);
    assert_eq!(bus.memory.trace.borrow().count, 0);
    metadata(&cpu, 0x4000);
    phase(&mut cpu, &mut bus, &mut state, 3, false);
    expected.sp = 0x7ffe;
    assert_eq!(cpu.regs(), expected);
    assert_eq!(
        bus.memory.trace.borrow().events[0],
        Some(Access::Write(0x7fff, 0x40))
    );
    phase(&mut cpu, &mut bus, &mut state, 3, true);
    expected.pc = 0x38;
    assert_eq!(cpu.regs(), expected);
    assert_eq!(
        bus.memory.trace.borrow().events[1],
        Some(Access::Write(0x7ffe, 0))
    );
    assert_eq!(bus.memory.trace.borrow().count, 2);
    assert_eq!(cpu.cycles() - cycles, 13);
    metadata(&cpu, 0x4000);
}

pub fn all_interrupt_services_match_literal_effects_and_steppers() {
    first_acceptance();
    let mut rows = 0;
    for service in SERVICES {
        for (pc, sp, r, iff2) in [
            (0x4000, 0x8000, 0xff, true),
            (0x1234, 1, 0x7f, false),
            (0xffff, 0, 0x80, true),
            (0, 0xffff, 0, false),
        ] {
            literal_case(service, pc, sp, r, iff2);
            rows += 1;
        }
    }
    assert_eq!(rows, 44);
}

fn service_fixture(service: Service, pc: u16, sp: u16, r: u8, iff2: bool) -> (Cpu, IrqBus) {
    let (mode, byte) = service.setup();
    let (mut cpu, mut bus) = fixture(mode, byte, pc, sp, r, 0x12);
    cpu.finish_guest_call(true, iff2, 0);
    cpu.resume();
    bus.memory.nmi = matches!(service, Service::Nmi);
    bus.memory.bytes[usize::from(pc)] = 0xdd;
    bus.memory.bytes[0x12ff] = 0x78;
    bus.memory.bytes[0x1300] = 0x56;
    (cpu, bus)
}

fn finish(cpu: &mut Cpu, bus: &mut IrqBus, state: &mut BusState, service: Service, from: usize) {
    for (index, &clock) in service.clocks().iter().enumerate().skip(from) {
        phase(cpu, bus, state, clock, index + 1 == service.clocks().len());
    }
}

fn trace(bus: &IrqBus) -> Vec<Option<Access>> {
    let trace = bus.memory.trace.borrow();
    trace.events[..trace.count].to_vec()
}

fn literal_case(service: Service, pc: u16, sp: u16, r: u8, iff2: bool) {
    let (mut cpu, mut bus) = service_fixture(service, pc, sp, r, iff2);
    let initial = cpu.regs();
    let mut expected = initial;
    expected.r = (r & 0x80) | (r.wrapping_add(1) & 0x7f);
    let mut aggregate = (cpu.clone(), bus.clone());
    let total: u32 = service.clocks().iter().sum();
    let instruction = aggregate.0.step_with_bus(&mut aggregate.1).unwrap();
    assert_eq!(
        (instruction.pc, instruction.opcode, instruction.cycles),
        (pc, 0xff, total)
    );
    let mut bounded = (cpu.clone(), bus.clone());
    let mut prefix = PrefixState::default();
    let step = bounded.0.step_bounded(&mut bounded.1, &mut prefix).unwrap();
    assert_eq!(
        (step.instruction, step.complete, prefix.pending()),
        (instruction, true, false)
    );
    assert_eq!(bounded, aggregate);
    let cycles = cpu.cycles();
    let mut accesses = Vec::new();
    let mut state = BusState::default();
    let mut polls = 0;
    for (index, &clock) in service.clocks().iter().enumerate() {
        phase(
            &mut cpu,
            &mut bus,
            &mut state,
            clock,
            index + 1 == service.clocks().len(),
        );
        match index {
            0 => {
                expected.sp = sp.wrapping_sub(1);
                if matches!(service, Service::Nmi) {
                    accesses.push(Some(Access::Read(pc, 0xdd)));
                }
                polls = bus.memory.polls.get();
                assert_eq!(
                    polls,
                    if matches!(service, Service::Nmi) {
                        1
                    } else {
                        2
                    }
                );
            }
            1 => {
                expected.sp = sp.wrapping_sub(2);
                accesses.push(Some(Access::Write(sp.wrapping_sub(1), (pc >> 8) as u8)));
            }
            2 => {
                accesses.push(Some(Access::Write(sp.wrapping_sub(2), pc as u8)));
                if !matches!(service, Service::Im2) {
                    expected.pc = service.target();
                }
            }
            3 => accesses.push(Some(Access::Read(0x12ff, 0x78))),
            4 => {
                accesses.push(Some(Access::Read(0x1300, 0x56)));
                expected.pc = 0x5678;
            }
            _ => unreachable!(),
        }
        assert_eq!(cpu.regs(), expected, "{service:?} phase {index}");
        assert_eq!(cpu.state(), CpuState::Running);
        assert_eq!(
            (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
            (false, matches!(service, Service::Nmi) && iff2)
        );
        assert_eq!(trace(&bus), accesses);
        assert_eq!(bus.memory.polls.get(), polls);
        assert_eq!(
            (bus.irq_acks, bus.memory.acknowledgements),
            if matches!(service, Service::Nmi) {
                (0, 1)
            } else {
                (1, 0)
            }
        );
        metadata(&cpu, pc);
    }
    assert_eq!(cpu.cycles() - cycles, u64::from(total));
    assert_eq!((cpu, bus), aggregate);
}

pub fn interrupt_vector_reads_and_stack_writes_are_live() {
    for (i, vector, high, sp, alias) in [
        (0x12, 0x12ff, 0x1300, 0x8000, false),
        (0xff, 0xffff, 0, 0x8000, false),
        (0xff, 0xffff, 0, 1, true),
    ] {
        let (mut cpu, mut bus) = fixture(0x5e, 0xff, 0x1234, sp, 0x7f, i);
        bus.memory.bytes[vector] = 0xa5;
        bus.memory.bytes[high] = 0x5a;
        let mut state = BusState::default();
        phase(&mut cpu, &mut bus, &mut state, 7, false);
        let polls = bus.memory.polls.get();
        bus.byte = 0;
        bus.memory.irq = false;
        bus.memory.nmi = true;
        phase(&mut cpu, &mut bus, &mut state, 3, false);
        assert_eq!(bus.memory.bytes[usize::from(sp.wrapping_sub(1))], 0x12);
        phase(&mut cpu, &mut bus, &mut state, 3, false);
        assert_eq!(bus.memory.bytes[usize::from(sp.wrapping_sub(2))], 0x34);
        assert_eq!(cpu.regs().pc, 0x1234);
        if !alias {
            bus.memory.bytes[vector] = 0x78;
        }
        phase(&mut cpu, &mut bus, &mut state, 3, false);
        assert_eq!(cpu.regs().pc, 0x1234);
        let low = if alias { 0x34 } else { 0x78 };
        bus.memory.bytes[vector] = 0x99;
        if !alias {
            bus.memory.bytes[high] = 0x56;
        }
        phase(&mut cpu, &mut bus, &mut state, 3, true);
        let upper = if alias { 0x12 } else { 0x56 };
        assert_eq!(cpu.regs().pc, u16::from_le_bytes([low, upper]));
        assert_eq!(
            trace(&bus),
            vec![
                Some(Access::Write(sp.wrapping_sub(1), 0x12)),
                Some(Access::Write(sp.wrapping_sub(2), 0x34)),
                Some(Access::Read(vector as u16, low)),
                Some(Access::Read(high as u16, upper))
            ]
        );
        assert_eq!(
            (
                bus.memory.polls.get(),
                bus.irq_acks,
                bus.memory.acknowledgements
            ),
            (polls, 1, 0)
        );
    }
}

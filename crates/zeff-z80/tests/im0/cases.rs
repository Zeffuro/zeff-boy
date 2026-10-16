#[path = "../multi_load/support.rs"]
#[allow(dead_code)]
mod support;

use support::{Access, Bus};
use zeff_z80::{BusState, Cpu, CpuState, CpuTrap, PrefixState, Registers, Z80Bus};

#[path = "lifecycle.rs"]
mod lifecycle;
pub use lifecycle::{
    im0_completed_codec_layout_and_tags, im0_default_reads_and_remaining_guards,
    im0_pending_clone_suspend_reset,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct IrqBus {
    memory: Bus,
    opcode: u8,
    operands: [u8; 2],
    reads: [Option<(u16, u8)>; 2],
    read_count: usize,
    acknowledgements: u32,
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
        self.acknowledgements += 1;
        self.opcode
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.memory.acknowledge_non_maskable_interrupt()
    }

    fn read_interrupt_instruction(&mut self, pc: u16) -> u8 {
        let value = self.operands[self.read_count];
        self.reads[self.read_count] = Some((pc, value));
        self.read_count += 1;
        value
    }
}

fn fixture(opcode: u8, pc: u16, sp: u16, r: u8) -> (Cpu, IrqBus) {
    fixture_flags(opcode, pc, sp, r, 0xa5)
}

fn fixture_flags(opcode: u8, pc: u16, sp: u16, r: u8, flags: u8) -> (Cpu, IrqBus) {
    let (mut cpu, mut memory) = support::seeded(0x12);
    memory.bytes[0x9000..0x9002].copy_from_slice(&[flags, 0x12]);
    let setup = [
        0x31,
        0x00,
        0x90,
        0xf1,
        0x31,
        sp as u8,
        (sp >> 8) as u8,
        0xed,
        0x46,
        0x3e,
        (r & 0x80) | (r.wrapping_sub(1) & 0x7f),
        0xed,
        0x4f,
        0xc3,
        pc as u8,
        (pc >> 8) as u8,
    ];
    let start = usize::from(cpu.regs().pc);
    memory.bytes[start..start + setup.len()].copy_from_slice(&setup);
    for _ in 0..7 {
        cpu.step_with_bus(&mut memory).unwrap();
        memory.clear_trace();
    }
    assert_eq!((cpu.regs().pc, cpu.regs().sp, cpu.regs().r), (pc, sp, r));
    assert_eq!(cpu.regs().f, flags);
    cpu.finish_guest_call(true, true, 0);
    cpu.resume();
    memory.irq = true;
    memory.polls.set(0);
    (
        cpu,
        IrqBus {
            memory,
            opcode,
            operands: [0x78, 0x56],
            reads: [None; 2],
            read_count: 0,
            acknowledgements: 0,
        },
    )
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

pub fn im0_nop_acceptance() {
    let (mut cpu, mut bus) = fixture(0, 0xffff, 0, 0xff);
    let mut expected = cpu.regs();
    expected.r = 0x80;
    let before = cpu.cycles();
    let mut state = BusState::default();
    let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
    assert_eq!(
        (
            step.instruction.pc,
            step.instruction.opcode,
            step.instruction.cycles
        ),
        (0xffff, 0xff, 6)
    );
    assert!(step.complete);
    assert!(!state.pending());
    assert_eq!(cpu.regs(), expected);
    assert_eq!(cpu.cycles() - before, 6);
    assert_eq!(cpu.state(), CpuState::Running);
    assert_eq!(cpu.trap(), None);
    assert_eq!(
        (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
        (false, false)
    );
    assert_eq!(
        (bus.acknowledgements, bus.memory.polls.get(), bus.read_count),
        (1, 2, 0)
    );
    assert_eq!(bus.reads, [None; 2]);
    assert_eq!(bus.memory.trace.borrow().count, 0);
    metadata(&cpu, 0xffff);
}

#[derive(Clone, Copy, Debug)]
enum Effect {
    Nop,
    Byte,
    Word,
    Jump(bool),
    Call(bool),
    Rst(u16),
}

#[derive(Clone, Copy, Debug)]
struct Case {
    opcode: u8,
    flags: u8,
    effect: Effect,
}

impl Case {
    fn clocks(self) -> &'static [u32] {
        match self.effect {
            Effect::Nop => &[6],
            Effect::Byte => &[6, 3],
            Effect::Word | Effect::Jump(_) | Effect::Call(false) => &[6, 3, 3],
            Effect::Call(true) => &[6, 3, 3, 1, 3, 3],
            Effect::Rst(_) => &[7, 3, 3],
        }
    }
}

fn cases() -> Vec<Case> {
    let mut cases = vec![Case {
        opcode: 0,
        flags: 0xa5,
        effect: Effect::Nop,
    }];
    for opcode in [0x06, 0x0e, 0x16, 0x1e, 0x26, 0x2e, 0x3e] {
        cases.push(Case {
            opcode,
            flags: 0xa5,
            effect: Effect::Byte,
        });
    }
    for opcode in [0x01, 0x11, 0x21, 0x31] {
        cases.push(Case {
            opcode,
            flags: 0xa5,
            effect: Effect::Word,
        });
    }
    cases.push(Case {
        opcode: 0xc3,
        flags: 0xa5,
        effect: Effect::Jump(true),
    });
    cases.push(Case {
        opcode: 0xcd,
        flags: 0xa5,
        effect: Effect::Call(true),
    });
    for (jump, call, true_flags, false_flags) in [
        (0xc2, 0xc4, 0x00, 0x40),
        (0xca, 0xcc, 0x40, 0x00),
        (0xd2, 0xd4, 0x00, 0x01),
        (0xda, 0xdc, 0x01, 0x00),
        (0xe2, 0xe4, 0x00, 0x04),
        (0xea, 0xec, 0x04, 0x00),
        (0xf2, 0xf4, 0x00, 0x80),
        (0xfa, 0xfc, 0x80, 0x00),
    ] {
        for (flags, taken) in [(true_flags, true), (false_flags, false)] {
            cases.push(Case {
                opcode: jump,
                flags,
                effect: Effect::Jump(taken),
            });
            cases.push(Case {
                opcode: call,
                flags,
                effect: Effect::Call(taken),
            });
        }
    }
    for (opcode, target) in [
        (0xc7, 0),
        (0xcf, 8),
        (0xd7, 0x10),
        (0xdf, 0x18),
        (0xe7, 0x20),
        (0xef, 0x28),
        (0xf7, 0x30),
        (0xff, 0x38),
    ] {
        cases.push(Case {
            opcode,
            flags: 0xa5,
            effect: Effect::Rst(target),
        });
    }
    assert_eq!(cases.len(), 54);
    cases
}

fn phase(cpu: &mut Cpu, bus: &mut IrqBus, state: &mut BusState, clocks: u32, complete: bool) {
    let step = cpu.step_bus_segment(bus, state).unwrap();
    assert_eq!(
        (step.instruction.cycles, step.complete, state.pending()),
        (clocks, complete, !complete)
    );
    assert_eq!(
        (step.instruction.pc, step.instruction.opcode),
        (cpu.last_opcode_pc(), 0xff)
    );
}

fn finish(cpu: &mut Cpu, bus: &mut IrqBus, state: &mut BusState, case: Case, from: usize) {
    for (index, &clock) in case.clocks().iter().enumerate().skip(from) {
        phase(cpu, bus, state, clock, index + 1 == case.clocks().len());
    }
}

fn trace(bus: &IrqBus) -> Vec<Option<Access>> {
    let trace = bus.memory.trace.borrow();
    trace.events[..trace.count].to_vec()
}

fn load(expected: &mut Registers, opcode: u8, word: bool) {
    match opcode {
        0x06 => expected.b = 0x78,
        0x0e => expected.c = 0x78,
        0x16 => expected.d = 0x78,
        0x1e => expected.e = 0x78,
        0x26 => expected.h = 0x78,
        0x2e => expected.l = 0x78,
        0x3e => expected.a = 0x78,
        0x01 if word => {
            expected.b = 0x56;
            expected.c = 0x78;
        }
        0x11 if word => {
            expected.d = 0x56;
            expected.e = 0x78;
        }
        0x21 if word => {
            expected.h = 0x56;
            expected.l = 0x78;
        }
        0x31 if word => expected.sp = 0x5678,
        _ => unreachable!(),
    }
}

pub fn im0_literals_match_all_steppers() {
    let mut rows = 0;
    for case in cases() {
        for (pc, sp, r, refreshed) in [
            (0x1234, 0x8000, 0x7f, 0x00),
            (0xfffe, 0xffff, 0xff, 0x80),
            (0xffff, 1, 0x80, 0x81),
            (0, 0, 0, 1),
        ] {
            literal_case(case, pc, sp, r, refreshed);
            rows += 1;
        }
    }
    assert_eq!(rows, 216);
}

fn literal_case(case: Case, pc: u16, sp: u16, r: u8, refreshed: u8) {
    let (mut cpu, mut bus) = fixture_flags(case.opcode, pc, sp, r, case.flags);
    bus.memory.bytes[usize::from(pc)] = 0xdd;
    bus.memory.bytes[usize::from(pc.wrapping_add(1))] = 0xed;
    let mut expected = cpu.regs();
    expected.r = refreshed;
    let mut aggregate = (cpu.clone(), bus.clone());
    let total: u32 = case.clocks().iter().sum();
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
    let before = cpu.cycles();
    let mut state = BusState::default();
    let mut accesses = Vec::new();
    let mut reads = [None; 2];
    for (index, &clock) in case.clocks().iter().enumerate() {
        phase(
            &mut cpu,
            &mut bus,
            &mut state,
            clock,
            index + 1 == case.clocks().len(),
        );
        match case.effect {
            Effect::Byte if index == 1 => {
                reads[0] = Some((pc, 0x78));
                load(&mut expected, case.opcode, false);
            }
            Effect::Word | Effect::Jump(_) | Effect::Call(_) if index == 1 => {
                reads[0] = Some((pc, 0x78))
            }
            Effect::Word if index == 2 => {
                reads[1] = Some((pc, 0x56));
                load(&mut expected, case.opcode, true);
            }
            Effect::Jump(taken) | Effect::Call(taken) if index == 2 => {
                reads[1] = Some((pc, 0x56));
                if taken && matches!(case.effect, Effect::Jump(_)) {
                    expected.pc = 0x5678;
                }
            }
            Effect::Call(true) if index == 3 => expected.sp = sp.wrapping_sub(1),
            Effect::Call(true) if index == 4 => {
                expected.sp = sp.wrapping_sub(2);
                accesses.push(Some(Access::Write(sp.wrapping_sub(1), (pc >> 8) as u8)));
            }
            Effect::Call(true) if index == 5 => {
                expected.pc = 0x5678;
                accesses.push(Some(Access::Write(sp.wrapping_sub(2), pc as u8)));
            }
            Effect::Rst(_) if index == 0 => expected.sp = sp.wrapping_sub(1),
            Effect::Rst(_) if index == 1 => {
                expected.sp = sp.wrapping_sub(2);
                accesses.push(Some(Access::Write(sp.wrapping_sub(1), (pc >> 8) as u8)));
            }
            Effect::Rst(target) if index == 2 => {
                expected.pc = target;
                accesses.push(Some(Access::Write(sp.wrapping_sub(2), pc as u8)));
            }
            _ => {}
        }
        assert_eq!(cpu.regs(), expected, "{case:?} phase {index}");
        assert_eq!(cpu.state(), CpuState::Running);
        assert_eq!(cpu.trap(), None);
        assert_eq!(
            (cpu.interrupts_enabled(), cpu.saved_interrupts_enabled()),
            (false, false)
        );
        assert_eq!(
            (
                bus.acknowledgements,
                bus.memory.polls.get(),
                bus.memory.acknowledgements
            ),
            (1, 2, 0)
        );
        assert_eq!(bus.reads, reads);
        assert_eq!(bus.read_count, reads.iter().flatten().count());
        assert_eq!(trace(&bus), accesses);
        metadata(&cpu, pc);
    }
    assert_eq!(cpu.cycles() - before, u64::from(total));
    assert_eq!((cpu, bus), aggregate);
}

pub fn im0_instruction_bytes_are_live_and_held() {
    for (opcode, effect) in [
        (0x3e, Effect::Byte),
        (0x21, Effect::Word),
        (0xc3, Effect::Jump(true)),
        (0xcd, Effect::Call(true)),
    ] {
        let case = Case {
            opcode,
            flags: 0xa5,
            effect,
        };
        let (mut cpu, mut bus) = fixture(opcode, 0xffff, 1, 0xff);
        bus.operands = [0xa5, 0x5a];
        let mut state = BusState::default();
        phase(&mut cpu, &mut bus, &mut state, 6, false);
        assert_eq!(bus.read_count, 0);
        bus.opcode = 0xdd;
        bus.memory.irq = false;
        bus.memory.nmi = true;
        bus.operands[0] = 0x78;
        phase(
            &mut cpu,
            &mut bus,
            &mut state,
            3,
            matches!(effect, Effect::Byte),
        );
        assert_eq!(bus.reads[0], Some((0xffff, 0x78)));
        if !matches!(effect, Effect::Byte) {
            bus.operands = [0x99, 0x56];
            phase(
                &mut cpu,
                &mut bus,
                &mut state,
                3,
                !matches!(effect, Effect::Call(true)),
            );
            assert_eq!(bus.reads, [Some((0xffff, 0x78)), Some((0xffff, 0x56))]);
            if matches!(effect, Effect::Call(true)) {
                bus.operands = [0, 0];
                finish(&mut cpu, &mut bus, &mut state, case, 3);
                assert_eq!(
                    trace(&bus),
                    vec![
                        Some(Access::Write(0, 0xff)),
                        Some(Access::Write(0xffff, 0xff))
                    ]
                );
            }
        }
        match effect {
            Effect::Byte => assert_eq!((cpu.regs().a, cpu.regs().pc), (0x78, 0xffff)),
            Effect::Word => assert_eq!((cpu.regs().hl(), cpu.regs().pc), (0x5678, 0xffff)),
            _ => assert_eq!(cpu.regs().pc, 0x5678),
        }
        assert_eq!(
            (
                bus.acknowledgements,
                bus.memory.polls.get(),
                bus.memory.acknowledgements
            ),
            (1, 2, 0)
        );
        metadata(&cpu, 0xffff);
    }
}

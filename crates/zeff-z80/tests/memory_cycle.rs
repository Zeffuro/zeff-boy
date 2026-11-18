#[allow(dead_code)]
#[path = "multi_load/support.rs"]
mod support;

use zeff_z80::BusState;

pub fn memory_cycle_classification() {
    ed_block_memory_phases();
    single_memory_phases();
    relative_phases_remain_unclassified();
    for (program, phases) in [
        (&[0x06, 0x55][..], &[false, true][..]),
        (&[0x21, 0x34, 0x12][..], &[false, true, true][..]),
        (&[0x22, 0, 0x80][..], &[false, true, true, true, true][..]),
        (&[0x2a, 0, 0x80][..], &[false, true, true, true, true][..]),
        (&[0x32, 0, 0x80][..], &[false, true, true, true][..]),
        (&[0x3a, 0, 0x80][..], &[false, true, true, true][..]),
        (&[0x36, 0x55][..], &[false, true, true][..]),
        (
            &[0xdd, 0x21, 0x34, 0x12][..],
            &[false, false, true, true][..],
        ),
        (
            &[0xfd, 0x22, 0, 0x80][..],
            &[false, false, true, true, true, true][..],
        ),
        (
            &[0xed, 0x43, 0, 0x80][..],
            &[false, false, true, true, true, true][..],
        ),
        (
            &[0xed, 0x4b, 0, 0x80][..],
            &[false, false, true, true, true, true][..],
        ),
        (&[0xe5][..], &[false, false, true, true][..]),
        (&[0xe1][..], &[false, true, true][..]),
        (&[0xc3, 0x34, 0x12][..], &[false, true, true][..]),
        (
            &[0xcd, 0x34, 0x12][..],
            &[false, true, true, false, true, true][..],
        ),
        (&[0xc9][..], &[false, true, true][..]),
        (&[0xcf][..], &[false, false, true, true][..]),
        (&[0xe3][..], &[false, true, false, true, false][..]),
        (&[0xdd, 0xe5][..], &[false, false, false, true, true][..]),
        (&[0xfd, 0xe1][..], &[false, false, true, true][..]),
        (&[0xed, 0x45][..], &[false, false, false, false][..]),
        (&[0x77][..], &[false, true][..]),
        (
            &[0xfd, 0x66, 0x1b][..],
            &[false, false, false, false, true][..],
        ),
        (
            &[0xdd, 0x66, 0x1b][..],
            &[false, false, false, false, true][..],
        ),
        (
            &[0xfd, 0x74, 0x1b][..],
            &[false, false, false, false, false][..],
        ),
        (
            &[0xfd, 0x86, 0x1b][..],
            &[false, false, false, false, false][..],
        ),
        (
            &[0xfd, 0x36, 0x1b, 0x55][..],
            &[false, false, false, false, false][..],
        ),
    ] {
        let (mut cpu, mut bus) = support::seeded(program[0]);
        let pc = usize::from(cpu.regs().pc);
        bus.bytes[pc..pc + program.len()].copy_from_slice(program);
        let mut state = BusState::default();
        for &memory in phases {
            let classified = state.next_memory_cycle();
            assert_eq!(classified, memory.then_some(3));
            bus.clear_trace();
            let segment = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            if memory {
                assert_eq!(segment.instruction.cycles, 3);
                assert_eq!(bus.trace.borrow().count, 1);
            }
        }
        assert!(!state.pending());
        assert_eq!(state.next_memory_cycle(), None);
    }
}

fn single_memory_phases() {
    use support::Access;
    let mut cases = 0;
    let mut accesses = [0; 2];
    for prefixes in [&[][..], &[0xdd][..], &[0xfd][..]] {
        for opcode in [
            0x02, 0x0a, 0x12, 0x1a, 0x06, 0x0e, 0x16, 0x1e, 0x26, 0x2e, 0x3e, 0xc6, 0xce, 0xd6,
            0xde, 0xe6, 0xee, 0xf6, 0xfe, 0x46, 0x4e, 0x56, 0x5e, 0x66, 0x6e, 0x7e, 0x70, 0x71,
            0x72, 0x73, 0x74, 0x75, 0x77, 0x86, 0x8e, 0x96, 0x9e, 0xa6, 0xae, 0xb6, 0xbe,
        ] {
            let immediate_load = opcode & 0xc7 == 0x06;
            if prefixes.is_empty() && immediate_load
                || !prefixes.is_empty() && (0x40..0xc0).contains(&opcode)
            {
                continue;
            }
            let (mut cpu, mut bus) = support::seeded(0x5a);
            let initial = cpu.regs();
            let pc = usize::from(initial.pc);
            bus.bytes[pc..pc + prefixes.len()].copy_from_slice(prefixes);
            bus.bytes[pc + prefixes.len()..pc + prefixes.len() + 2]
                .copy_from_slice(&[opcode, 0x36]);
            bus.bytes[usize::from(initial.bc())] = 0x11;
            bus.bytes[usize::from(initial.de())] = 0x22;
            bus.bytes[usize::from(initial.hl())] = 0x33;
            let expected = match opcode {
                0x02 => Access::Write(initial.bc(), initial.a),
                0x12 => Access::Write(initial.de(), initial.a),
                0x0a => Access::Read(initial.bc(), 0x11),
                0x1a => Access::Read(initial.de(), 0x22),
                0x70..=0x75 | 0x77 => Access::Write(
                    initial.hl(),
                    [
                        initial.b, initial.c, initial.d, initial.e, initial.h, initial.l, 0,
                        initial.a,
                    ][usize::from(opcode & 7)],
                ),
                0x40..=0xbf => Access::Read(initial.hl(), 0x33),
                _ => Access::Read(initial.pc + prefixes.len() as u16 + 1, 0x36),
            };
            let mut aggregate_cpu = cpu.clone();
            let mut aggregate_bus = bus.clone();
            aggregate_cpu.step_with_bus(&mut aggregate_bus).unwrap();
            let mut state = BusState::default();
            for _ in 0..=prefixes.len() {
                assert_eq!(state.next_memory_cycle(), None);
                let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                assert_eq!(step.instruction.cycles, 4);
                assert!(!step.complete);
            }
            assert_eq!(state.next_memory_cycle(), Some(3));
            bus.clear_trace();
            let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert_eq!(step.instruction.cycles, 3);
            assert!(step.complete);
            assert_eq!(bus.trace.borrow().count, 1);
            assert_eq!(bus.trace.borrow().events[0], Some(expected));
            accesses[usize::from(matches!(expected, Access::Write(_, _)))] += 1;
            assert_eq!(cpu, aggregate_cpu);
            assert_eq!(bus.bytes, aggregate_bus.bytes);
            assert_eq!(state.next_memory_cycle(), None);
            cases += 1;
        }
    }
    assert_eq!(cases, 72);
    assert_eq!(accesses, [59, 13]);
}

fn relative_phases_remain_unclassified() {
    for prefixes in [&[][..], &[0xdd][..], &[0xfd][..]] {
        for opcode in [0x10, 0x18, 0x20, 0x28, 0x30, 0x38] {
            for (zero, carry, b) in [
                (false, false, 1),
                (false, true, 2),
                (true, false, 2),
                (true, true, 1),
            ] {
                let (mut cpu, mut bus) = support::seeded(0x5a);
                let pc = usize::from(cpu.regs().pc);
                bus.bytes[pc..pc + 4].copy_from_slice(&[
                    0x06,
                    b,
                    if zero { 0xaf } else { 0xb7 },
                    if carry { 0x37 } else { 0x00 },
                ]);
                for _ in 0..3 {
                    cpu.step_with_bus(&mut bus).unwrap();
                    bus.clear_trace();
                }
                let pc = usize::from(cpu.regs().pc);
                bus.bytes[pc..pc + prefixes.len()].copy_from_slice(prefixes);
                bus.bytes[pc + prefixes.len()..pc + prefixes.len() + 2]
                    .copy_from_slice(&[opcode, 0]);
                let mut state = BusState::default();
                loop {
                    assert_eq!(state.next_memory_cycle(), None);
                    let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                    if step.complete {
                        break;
                    }
                }
            }
        }
    }
}

fn ed_block_memory_phases() {
    use support::Access;
    let mut qualified = [0; 2];
    for prefixes in [&[][..], &[0xdd][..], &[0xfd][..]] {
        for (opcodes, clocks, memory) in [
            (
                [0xa0, 0xa8, 0xb0, 0xb8],
                &[4, 4, 3, 3, 2][..],
                &[0, 0, 1, 2, 0][..],
            ),
            (
                [0xa1, 0xa9, 0xb1, 0xb9],
                &[4, 4, 3, 5][..],
                &[0, 0, 1, 0][..],
            ),
            (
                [0xa2, 0xaa, 0xb2, 0xba],
                &[4, 4, 1, 4, 3][..],
                &[0, 0, 0, 0, 2][..],
            ),
            (
                [0xa3, 0xab, 0xb3, 0xbb],
                &[4, 4, 1, 3, 4][..],
                &[0, 0, 0, 1, 0][..],
            ),
        ] {
            for opcode in opcodes {
                let (mut cpu, mut bus) = support::seeded(0x5a);
                let initial = cpu.regs();
                let pc = usize::from(initial.pc);
                bus.bytes[pc..pc + prefixes.len()].copy_from_slice(prefixes);
                bus.bytes[pc + prefixes.len()..pc + prefixes.len() + 2]
                    .copy_from_slice(&[0xed, opcode]);
                bus.bytes[usize::from(initial.hl())] = 6;
                let mut phases = vec![(4, 0); prefixes.len()];
                phases.extend(clocks.iter().copied().zip(memory.iter().copied()));
                if opcode & 0x10 != 0 {
                    phases.push((5, 0));
                }
                let mut state = BusState::default();
                for (index, &(clocks, kind)) in phases.iter().enumerate() {
                    assert_eq!(state.next_memory_cycle(), (kind != 0).then_some(3));
                    bus.clear_trace();
                    let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                    assert_eq!(step.instruction.cycles, clocks);
                    assert_eq!(step.complete, index + 1 == phases.len());
                    if kind != 0 {
                        let expected = if kind == 1 {
                            Access::Read(initial.hl(), 6)
                        } else if opcode & 3 == 0 {
                            Access::Write(initial.de(), 6)
                        } else {
                            Access::Write(initial.hl(), 0)
                        };
                        assert_eq!(bus.trace.borrow().count, 1);
                        assert_eq!(bus.trace.borrow().events[0], Some(expected));
                        qualified[kind - 1] += 1;
                    }
                }
                assert!(!state.pending());
                assert_eq!(state.next_memory_cycle(), None);
            }
        }
    }
    assert_eq!(qualified, [36, 24]);
}

#[test]
fn next_memory_cycle_classifies_owned_memory_phases() {
    memory_cycle_classification();
}

pub fn memory_cycle_excludes_interrupt_origin() {
    for nmi in [false, true] {
        let mut cpu = zeff_z80::Cpu::new_with_reset(zeff_z80::ResetState::new(0x100, 0x8000));
        let mut bus = support::Bus::new();
        bus.bytes[0x100..0x102].copy_from_slice(&[0xfb, 0x76]);
        let mut state = BusState::default();
        for _ in 0..2 {
            cpu.step_bus_segment(&mut bus, &mut state).unwrap();
        }
        bus.nmi = nmi;
        bus.irq = !nmi;
        for (index, clocks) in [if nmi { 5 } else { 7 }, 3, 3].into_iter().enumerate() {
            assert_eq!(state.next_memory_cycle(), None);
            let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert_eq!(step.instruction.cycles, clocks);
            assert_eq!(step.complete, index == 2);
        }
        assert_eq!(cpu.regs().pc, if nmi { 0x66 } else { 0x38 });
        assert_eq!(cpu.regs().sp, 0x7ffe);
        assert_eq!(state.next_memory_cycle(), None);
    }
}

#[test]
fn interrupt_stack_phases_remain_unclassified() {
    memory_cycle_excludes_interrupt_origin();
}

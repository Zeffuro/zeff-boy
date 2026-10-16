#[allow(dead_code)]
#[path = "../multi_load/support.rs"]
mod support;
use support::{Access, Bus, seeded};
use zeff_z80::{BusState, Cpu, PrefixState};

const OPCODES: [u8; 25] = [
    0x34, 0x35, 0x36, 0x46, 0x4e, 0x56, 0x5e, 0x66, 0x6e, 0x7e, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75,
    0x77, 0x86, 0x8e, 0x96, 0x9e, 0xa6, 0xae, 0xb6, 0xbe,
];

fn fixture(prefix: u8, opcode: u8, base: u16, displacement: u8, value: u8) -> (Cpu, Bus) {
    let (mut cpu, mut bus) = seeded(0x80);
    let pc = usize::from(cpu.regs().pc);
    let [low, high] = base.to_le_bytes();
    bus.bytes[pc..pc + 4].copy_from_slice(&[prefix, 0x21, low, high]);
    cpu.step_with_bus(&mut bus).unwrap();
    bus.clear_trace();
    let pc = usize::from(cpu.regs().pc);
    bus.bytes[pc..pc + 4].copy_from_slice(&[prefix, opcode, displacement, value]);
    let address = base.wrapping_add_signed(i16::from(displacement as i8));
    bus.bytes[usize::from(address)] = value;
    (cpu, bus)
}

fn schedule(opcode: u8) -> &'static [(u32, usize)] {
    match opcode {
        0x34 | 0x35 => &[(4, 1), (4, 1), (3, 1), (5, 0), (4, 1), (3, 1)],
        0x36 => &[(4, 1), (4, 1), (3, 1), (5, 1), (3, 1)],
        _ => &[(4, 1), (4, 1), (3, 1), (5, 0), (3, 1)],
    }
}

fn step(
    cpu: &mut Cpu,
    bus: &mut Bus,
    state: &mut BusState,
    clocks: u32,
    accesses: usize,
    complete: bool,
) {
    let before = bus.trace.borrow().count;
    let result = cpu.step_bus_segment(bus, state).unwrap();
    assert_eq!(result.instruction.cycles, clocks);
    assert_eq!(result.complete, complete);
    assert_eq!(state.pending(), !complete);
    assert_eq!(bus.trace.borrow().count, before + accesses);
}

fn finish(cpu: &mut Cpu, bus: &mut Bus, state: &mut BusState, opcode: u8, from: usize) {
    let phases = schedule(opcode);
    for (index, &(clocks, accesses)) in phases.iter().enumerate().skip(from) {
        step(cpu, bus, state, clocks, accesses, index + 1 == phases.len());
    }
}

pub fn all_indexed_memory_forms_match_legacy() {
    let mut encodings = 0;
    for prefix in [0xdd, 0xfd] {
        for opcode in OPCODES {
            encodings += 1;
            for base in [0u16, 0x8000, 0xffff] {
                for displacement in [0, 1, 0x7f, 0x80, 0xff] {
                    for value in [0, 1, 0x7f, 0x80, 0x81, 0xff] {
                        let (mut cpu, mut bus) = fixture(prefix, opcode, base, displacement, value);
                        let start = cpu.regs().pc;
                        let cycles = cpu.cycles();
                        let address = base.wrapping_add_signed(i16::from(displacement as i8));
                        let mut legacy = (cpu.clone(), bus.clone());
                        let mut bounded = (cpu.clone(), bus.clone(), PrefixState::default());
                        let mut state = BusState::default();
                        finish(&mut cpu, &mut bus, &mut state, opcode, 0);
                        let whole = legacy.0.step_with_bus(&mut legacy.1).unwrap();
                        assert!(
                            !bounded
                                .0
                                .step_bounded(&mut bounded.1, &mut bounded.2)
                                .unwrap()
                                .complete
                        );
                        assert!(
                            bounded
                                .0
                                .step_bounded(&mut bounded.1, &mut bounded.2)
                                .unwrap()
                                .complete
                        );
                        assert_eq!(cpu.cycles() - cycles, if opcode < 0x36 { 23 } else { 19 });
                        assert_eq!(cpu.cycles() - cycles, u64::from(whole.cycles));
                        assert_eq!((cpu.clone(), bus.clone()), legacy);
                        assert_eq!((cpu.clone(), bus.clone()), (bounded.0, bounded.1));
                        assert_eq!(cpu.regs().pc, start + if opcode == 0x36 { 4 } else { 3 });
                        assert_eq!(cpu.last_m1_fetch_count(), 2);
                        let trace = bus.trace.borrow();
                        assert_eq!(trace.events[0], Some(Access::Read(start, prefix)));
                        assert_eq!(trace.events[1], Some(Access::Read(start + 1, opcode)));
                        assert_eq!(trace.events[2], Some(Access::Read(start + 2, displacement)));
                        let access = trace.events[trace.count - 1].unwrap();
                        assert!(
                            matches!(access, Access::Read(a, _) | Access::Write(a, _) if a == address)
                        );
                    }
                }
            }
        }
    }
    assert_eq!(encodings, 50);
}

pub fn indexed_effects_match_literal_results() {
    for prefix in [0xdd, 0xfd] {
        for (opcode, value, expected_a, expected_f) in [
            (0x86, 0x81, 0x01, 0x05),
            (0x8e, 0x81, 0x02, 0x05),
            (0x96, 0x81, 0xff, 0xbb),
            (0x9e, 0x81, 0xfe, 0xbb),
            (0xa6, 0x81, 0x80, 0x90),
            (0xae, 0x81, 0x01, 0x00),
            (0xb6, 0x81, 0x81, 0x84),
            (0xbe, 0x81, 0x80, 0x93),
            (0x34, 0x7f, 0x80, 0x95),
            (0x35, 0x80, 0x80, 0x3f),
        ] {
            let (mut cpu, mut bus) = fixture(prefix, opcode, 0x8000, 0xff, value);
            finish(&mut cpu, &mut bus, &mut BusState::default(), opcode, 0);
            assert_eq!((cpu.regs().a, cpu.regs().f), (expected_a, expected_f));
            if opcode == 0x34 || opcode == 0x35 {
                assert_eq!(bus.bytes[0x7fff], if opcode == 0x34 { 0x80 } else { 0x7f });
            }
        }
        for (opcode, value) in [
            (0x70, 0x10),
            (0x71, 0x12),
            (0x72, 0x20),
            (0x73, 0x23),
            (0x74, 0x34),
            (0x75, 0x56),
            (0x77, 0x80),
            (0x36, 0xa5),
        ] {
            let (mut cpu, mut bus) = fixture(prefix, opcode, 0x8000, 1, 0xa5);
            finish(&mut cpu, &mut bus, &mut BusState::default(), opcode, 0);
            assert_eq!(bus.bytes[0x8001], value);
            assert_eq!(cpu.regs().f, 1);
        }
        for opcode in [0x46, 0x4e, 0x56, 0x5e, 0x66, 0x6e, 0x7e] {
            let (mut cpu, mut bus) = fixture(prefix, opcode, 0x8000, 0x80, 0xa5);
            let mut expected = cpu.regs();
            match opcode {
                0x46 => expected.b = 0xa5,
                0x4e => expected.c = 0xa5,
                0x56 => expected.d = 0xa5,
                0x5e => expected.e = 0xa5,
                0x66 => expected.h = 0xa5,
                0x6e => expected.l = 0xa5,
                0x7e => expected.a = 0xa5,
                _ => unreachable!(),
            }
            expected.pc += 3;
            expected.r = (expected.r & 0x80) | (expected.r.wrapping_add(2) & 0x7f);
            finish(&mut cpu, &mut bus, &mut BusState::default(), opcode, 0);
            assert_eq!(cpu.regs(), expected);
        }
    }
}

pub fn indexed_operands_are_live_until_their_phase() {
    for prefix in [0xdd, 0xfd] {
        for opcode in [0x7e, 0x86, 0x34, 0x35, 0x36, 0x74] {
            let (mut cpu, mut bus) = fixture(prefix, opcode, 0x8000, 1, 0x55);
            let pc = usize::from(cpu.regs().pc);
            let mut state = BusState::default();
            step(&mut cpu, &mut bus, &mut state, 4, 1, false);
            step(&mut cpu, &mut bus, &mut state, 4, 1, false);
            bus.bytes[pc + 2] = 0xff;
            step(&mut cpu, &mut bus, &mut state, 3, 1, false);
            bus.bytes[pc + 2] = 0x80;
            bus.bytes[pc + 3] = 0xa5;
            step(
                &mut cpu,
                &mut bus,
                &mut state,
                5,
                usize::from(opcode == 0x36),
                false,
            );
            bus.bytes[pc + 3] = 0;
            bus.bytes[0x7fff] = 0x81;
            step(
                &mut cpu,
                &mut bus,
                &mut state,
                if opcode < 0x36 { 4 } else { 3 },
                1,
                opcode >= 0x36,
            );
            if opcode < 0x36 {
                assert_eq!(cpu.regs().f, 1);
                bus.bytes[0x7fff] = 0;
                step(&mut cpu, &mut bus, &mut state, 3, 1, true);
            }
            match opcode {
                0x7e => assert_eq!(cpu.regs().a, 0x81),
                0x86 => assert_eq!((cpu.regs().a, cpu.regs().f), (1, 5)),
                0x34 => assert_eq!(bus.bytes[0x7fff], 0x82),
                0x35 => assert_eq!(bus.bytes[0x7fff], 0x80),
                0x36 => assert_eq!(bus.bytes[0x7fff], 0xa5),
                0x74 => assert_eq!(bus.bytes[0x7fff], 0x34),
                _ => unreachable!(),
            }
            assert_eq!(bus.bytes[0x8001], 0x55);
        }
    }
}

pub fn indexed_pending_phases_clone_suspend_and_reset() {
    for prefix in [0xdd, 0xfd] {
        for opcode in OPCODES {
            for count in 1..schedule(opcode).len() {
                let (mut cpu, mut bus) = fixture(prefix, opcode, 0xffff, 1, 0x81);
                let mut state = BusState::default();
                for &(clocks, accesses) in &schedule(opcode)[..count] {
                    step(&mut cpu, &mut bus, &mut state, clocks, accesses, false);
                }
                cpu.suspend();
                let saved = (cpu.clone(), bus.clone(), state);
                assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
                assert_eq!((cpu.clone(), bus.clone(), state), saved);
                cpu.resume();
                let mut copied = (cpu.clone(), bus.clone(), state);
                finish(&mut cpu, &mut bus, &mut state, opcode, count);
                finish(&mut copied.0, &mut copied.1, &mut copied.2, opcode, count);
                assert_eq!((cpu, bus, state), copied);
                let (mut reset_cpu, mut reset_bus, _) = saved;
                let data = reset_bus.bytes[0];
                reset_cpu.reset();
                let mut reset_state = BusState::default();
                reset_bus.clear_trace();
                reset_bus.bytes[0x100] = 0;
                step(&mut reset_cpu, &mut reset_bus, &mut reset_state, 4, 1, true);
                assert_eq!(reset_bus.bytes[0], data);
                assert_eq!(reset_cpu.regs().pc, 0x101);
            }
        }
    }
}

pub fn indexed_prefixes_and_interrupts_preserve_boundaries() {
    for opcode in OPCODES {
        let (mut cpu, mut bus) = fixture(0xfd, opcode, 0x8000, 0xff, 0x81);
        let pc = usize::from(cpu.regs().pc);
        bus.bytes[pc..pc + 7].copy_from_slice(&[0xfb, 0xdd, 0xfd, opcode, 0xff, 0x81, 0]);
        cpu.step_with_bus(&mut bus).unwrap();
        bus.clear_trace();
        let mut legacy = (cpu.clone(), bus.clone());
        let mut state = BusState::default();
        step(&mut cpu, &mut bus, &mut state, 4, 1, false);
        step(&mut cpu, &mut bus, &mut state, 4, 1, false);
        let polls = bus.polls.get();
        bus.irq = true;
        finish(&mut cpu, &mut bus, &mut state, opcode, 1);
        assert_eq!(bus.polls.get(), polls);
        assert!(!cpu.last_step_was_interrupt());
        legacy.0.step_with_bus(&mut legacy.1).unwrap();
        assert_eq!(cpu, legacy.0);
        bus.irq = false;
        assert_eq!(bus, legacy.1);
        bus.clear_trace();
        bus.irq = true;
        for (index, clocks) in [7, 3, 3].into_iter().enumerate() {
            let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert_eq!(step.instruction.cycles, clocks);
            assert_eq!(step.complete, index == 2);
        }
        assert!(cpu.last_step_was_interrupt());
    }
}

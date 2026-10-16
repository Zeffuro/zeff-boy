#[path = "../multi_load/support.rs"]
mod support;

use support::{Access, Bus, seeded};
use zeff_z80::{BusState, Cpu, PrefixState, Registers};

const OPCODES: [u8; 14] = [
    0x42, 0x52, 0x62, 0x72, 0x4a, 0x5a, 0x6a, 0x7a, 0x47, 0x4f, 0x57, 0x5f, 0x67, 0x6f,
];

fn nibble(opcode: u8) -> bool {
    matches!(opcode, 0x67 | 0x6f)
}

fn schedule(opcode: u8) -> &'static [u32] {
    match opcode {
        0x67 | 0x6f => &[4, 4, 3, 4, 3],
        0x47 | 0x4f | 0x57 | 0x5f => &[4, 4, 1],
        _ => &[4, 4, 4, 3],
    }
}

fn fixture(
    prefix: Option<u8>,
    opcode: u8,
    af: [u8; 2],
    pairs: [u16; 4],
    special: [u8; 2],
) -> (Cpu, Bus) {
    let (mut cpu, mut bus) = seeded(0x12);
    let mut program = Vec::new();
    for (opcode, value) in [(0x31, 0x9000), (0x01, pairs[0]), (0x11, pairs[1])] {
        program.push(opcode);
        program.extend(value.to_le_bytes());
    }
    program.extend([0x3e, special[0], 0xed, 0x47, 0x3e, special[1], 0xed, 0x4f]);
    program.extend([0x21, af[1], af[0], 0xe5, 0xf1]);
    for (opcode, value) in [(0x21, pairs[2]), (0x31, pairs[3]), (0xc3, 0x4000)] {
        program.push(opcode);
        program.extend(value.to_le_bytes());
    }
    let setup = usize::from(cpu.regs().pc);
    bus.bytes[setup..setup + program.len()].copy_from_slice(&program);
    while cpu.regs().pc != 0x4000 {
        cpu.step_with_bus(&mut bus).unwrap();
        bus.clear_trace();
    }
    let at = 0x4000 + usize::from(prefix.is_some());
    if let Some(prefix) = prefix {
        bus.bytes[0x4000] = prefix;
    }
    bus.bytes[at..at + 2].copy_from_slice(&[0xed, opcode]);
    (cpu, bus)
}

fn phase(
    cpu: &mut Cpu,
    bus: &mut Bus,
    state: &mut BusState,
    clocks: u32,
    complete: bool,
    access: bool,
) {
    if access {
        support::phase(cpu, bus, state, clocks, complete);
        return;
    }
    let before = bus.trace.borrow().count;
    let step = cpu.step_bus_segment(bus, state).unwrap();
    assert_eq!(step.instruction.cycles, clocks);
    assert_eq!(step.complete, complete);
    assert_eq!(state.pending(), !complete);
    assert_eq!(bus.trace.borrow().count, before + usize::from(access));
}

fn literal_case(
    mut cpu: Cpu,
    mut bus: Bus,
    prefix: Option<u8>,
    opcode: u8,
    mut expected: Registers,
    memory: Option<(u8, u8)>,
) {
    let initial = cpu.regs();
    let ed = 0x4000 + u16::from(prefix.is_some());
    expected.pc = ed + 2;
    if opcode != 0x4f {
        expected.r =
            (initial.r & 0x80) | (initial.r.wrapping_add(2 + u8::from(prefix.is_some())) & 0x7f);
    }
    let mut expected_memory = bus.bytes.clone();
    if let Some((input, output)) = memory {
        bus.bytes[usize::from(initial.hl())] = input;
        expected_memory[usize::from(initial.hl())] = output;
    }
    let mut accesses = Vec::new();
    for at in 0x4000..ed + 2 {
        accesses.push(Some(Access::Read(at, bus.bytes[usize::from(at)])));
    }
    if let Some((input, output)) = memory {
        accesses.extend([
            Some(Access::Read(initial.hl(), input)),
            Some(Access::Write(initial.hl(), output)),
        ]);
    }
    let mut legacy = (cpu.clone(), bus.clone());
    let total = legacy.0.step_with_bus(&mut legacy.1).unwrap().cycles;
    assert_eq!(
        total,
        schedule(opcode).iter().sum::<u32>() + 4 * u32::from(prefix.is_some())
    );
    let mut bounded = (cpu.clone(), bus.clone());
    let mut prefix_state = PrefixState::default();
    let mut bounded_clocks = 0;
    loop {
        let step = bounded
            .0
            .step_bounded(&mut bounded.1, &mut prefix_state)
            .unwrap();
        bounded_clocks += step.instruction.cycles;
        if step.complete {
            break;
        }
    }
    assert_eq!(bounded_clocks, total);
    assert_eq!(bounded, legacy);
    let start_cycles = cpu.cycles();
    let mut state = BusState::default();
    if prefix.is_some() {
        phase(&mut cpu, &mut bus, &mut state, 4, false, true);
    }
    for (i, &clock) in schedule(opcode).iter().enumerate() {
        let complete = i + 1 == schedule(opcode).len();
        phase(
            &mut cpu,
            &mut bus,
            &mut state,
            clock,
            complete,
            i < 2 || nibble(opcode) && i != 3,
        );
        assert_eq!(state.prefix_pending(), prefix.is_some() && !complete);
        if !complete {
            let mut pending = initial;
            let fetched = (i + 1).min(2) as u8 + u8::from(prefix.is_some());
            pending.pc += u16::from(fetched);
            pending.r = (initial.r & 0x80) | (initial.r.wrapping_add(fetched) & 0x7f);
            assert_eq!(cpu.regs(), pending, "{prefix:?} {opcode:02x} phase {i}");
            if let Some((input, _)) = memory {
                assert_eq!(bus.bytes[usize::from(initial.hl())], input);
            }
        }
    }
    assert_eq!(cpu.regs(), expected, "{prefix:?} {opcode:02x}");
    assert_eq!(cpu.cycles() - start_cycles, u64::from(total));
    assert_eq!(bus.bytes, expected_memory);
    assert_eq!(
        &bus.trace.borrow().events[..accesses.len()],
        accesses.as_slice()
    );
    assert_eq!(bus.trace.borrow().count, accesses.len());
    assert_eq!((cpu, bus), legacy);
}

pub fn all_ed_internal_forms_match_literal_effects_and_steppers() {
    let arithmetic = [
        (0x42, [(0x7ffe, 0x2a), (0x7ffd, 0x2a)]),
        (0x52, [(0xffff, 0xaf), (0xfffe, 0xaf)]),
        (0x62, [(0x0000, 0x42), (0xffff, 0xbb)]),
        (0x72, [(0x8000, 0x87), (0x7fff, 0x3b)]),
        (0x4a, [(0x8000, 0x94), (0x8001, 0x94)]),
        (0x5a, [(0xffff, 0xa8), (0x0000, 0x51)]),
        (0x6a, [(0xfffe, 0xbc), (0xffff, 0xbc)]),
        (0x7a, [(0x7ffe, 0x39), (0x7fff, 0x39)]),
    ];
    let nibbles = [
        (0x67, [0x12, 1], 0x34, 0x14, 0x23, 0x05),
        (0x6f, [0x12, 1], 0x34, 0x13, 0x42, 0x01),
        (0x67, [0x00, 0xfe], 0x00, 0x00, 0x00, 0x44),
        (0x6f, [0x00, 0xff], 0x00, 0x00, 0x00, 0x45),
        (0x67, [0xa5, 0xff], 0xf8, 0xa8, 0x5f, 0xa9),
        (0x6f, [0x85, 0xfe], 0x34, 0x83, 0x45, 0x80),
    ];
    let mut forms = 0;
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for (opcode, results) in arithmetic {
            forms += 1;
            for (carry, (hl, flags)) in results.into_iter().enumerate() {
                let (cpu, bus) = fixture(
                    prefix,
                    opcode,
                    [0x12, 0xfe | carry as u8],
                    [1, 0x8000, 0x7fff, 0xffff],
                    [0x80, 0xf8],
                );
                let mut expected = cpu.regs();
                (expected.h, expected.l) = ((hl >> 8) as u8, hl as u8);
                expected.f = flags;
                literal_case(cpu, bus, prefix, opcode, expected, None);
            }
        }
        for (opcode, af, input, a, output, flags) in nibbles {
            if af == [0x12, 1] {
                forms += 1;
            }
            for address in [0, 0x7000, 0xffff] {
                let (cpu, bus) = fixture(
                    prefix,
                    opcode,
                    af,
                    [1, 0x8000, address, 0xffff],
                    [0x80, 0xf8],
                );
                let mut expected = cpu.regs();
                expected.a = a;
                expected.f = flags;
                literal_case(cpu, bus, prefix, opcode, expected, Some((input, output)));
            }
        }
        for opcode in [0x47, 0x4f, 0x57, 0x5f] {
            forms += 1;
            let (cpu, bus) = fixture(
                prefix,
                opcode,
                [0xa8, 0xd7],
                [1, 0x8000, 0x7000, 0xffff],
                [0x80, 0xf8],
            );
            let mut expected = cpu.regs();
            match opcode {
                0x47 => expected.i = 0xa8,
                0x4f => expected.r = 0xa8,
                0x57 => {
                    expected.a = 0x80;
                    expected.f = 0x81;
                }
                _ => {
                    expected.a = 0x80 + u8::from(prefix.is_some());
                    expected.f = 0x81;
                }
            }
            literal_case(cpu, bus, prefix, opcode, expected, None);
        }
    }
    assert_eq!(forms, 42);
}

fn phases(prefix: Option<u8>, opcode: u8) -> Vec<(u32, bool)> {
    prefix
        .into_iter()
        .map(|_| (4, true))
        .chain(
            schedule(opcode)
                .iter()
                .enumerate()
                .map(|(i, &clock)| (clock, i < 2 || nibble(opcode) && i != 3)),
        )
        .collect()
}

fn finish(cpu: &mut Cpu, bus: &mut Bus, state: &mut BusState, clocks: &[(u32, bool)], from: usize) {
    for (i, &(clock, access)) in clocks.iter().enumerate().skip(from) {
        phase(cpu, bus, state, clock, i + 1 == clocks.len(), access);
    }
}

pub fn ed_internal_live_terminal_and_nibble_latches() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        let ed = 0x4000 + usize::from(prefix.is_some());
        for opcode in OPCODES {
            for address in [0x7000, ed as u16 + 1, 0xffff] {
                let (mut cpu, mut bus) = fixture(
                    prefix,
                    0,
                    [0x12, 1],
                    [1, 0x8000, address, 0xffff],
                    [0x80, 0xf8],
                );
                let clocks = phases(prefix, opcode);
                let mut state = BusState::default();
                let terminal = 1 + usize::from(prefix.is_some());
                for &(clock, access) in &clocks[..terminal] {
                    phase(&mut cpu, &mut bus, &mut state, clock, false, access);
                }
                bus.bytes[ed + 1] = opcode;
                phase(&mut cpu, &mut bus, &mut state, 4, false, true);
                bus.bytes[ed + 1] = 0;
                if nibble(opcode) {
                    bus.bytes[usize::from(address)] = 0x34;
                    phase(&mut cpu, &mut bus, &mut state, 3, false, true);
                    bus.bytes[usize::from(address)] = 0xab;
                    let regs = cpu.regs();
                    let trace = bus.trace.borrow().clone();
                    phase(&mut cpu, &mut bus, &mut state, 4, false, false);
                    assert_eq!(cpu.regs(), regs);
                    assert_eq!(*bus.trace.borrow(), trace);
                    assert_eq!(bus.bytes[usize::from(address)], 0xab);
                    phase(&mut cpu, &mut bus, &mut state, 3, true, true);
                    assert_eq!(
                        (cpu.regs().a, cpu.regs().f, bus.bytes[usize::from(address)]),
                        if opcode == 0x67 {
                            (0x14, 5, 0x23)
                        } else {
                            (0x13, 1, 0x42)
                        }
                    );
                    assert_eq!(
                        bus.trace.borrow().events[terminal + 1],
                        Some(Access::Read(address, 0x34))
                    );
                    assert_eq!(
                        bus.trace.borrow().events[terminal + 2],
                        Some(Access::Write(
                            address,
                            if opcode == 0x67 { 0x23 } else { 0x42 }
                        ))
                    );
                } else {
                    let before = cpu.regs();
                    finish(&mut cpu, &mut bus, &mut state, &clocks, terminal + 1);
                    match opcode {
                        0x47 => assert_eq!(cpu.regs().i, 0x12),
                        0x4f => assert_eq!(cpu.regs().r, 0x12),
                        0x57 => assert_eq!(cpu.regs().a, 0x80),
                        0x5f => assert_eq!(cpu.regs().a, before.r),
                        _ => assert_ne!(cpu.regs().f, 1),
                    }
                }
            }
            let (mut cpu, mut bus) = fixture(
                prefix,
                opcode,
                [0x12, 1],
                [1, 0x8000, 0x7000, 0xffff],
                [0x80, 0xf8],
            );
            let mut state = BusState::default();
            if prefix.is_some() {
                phase(&mut cpu, &mut bus, &mut state, 4, false, true);
            }
            phase(&mut cpu, &mut bus, &mut state, 4, false, true);
            bus.bytes[ed + 1] = 0;
            phase(&mut cpu, &mut bus, &mut state, 4, true, true);
            assert_eq!(
                (cpu.regs().a, cpu.regs().f, cpu.regs().hl()),
                (0x12, 1, 0x7000)
            );
        }
    }
}

pub fn ed_internal_pending_clone_suspend_reset() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            let clocks = phases(prefix, opcode);
            for count in 1..clocks.len() {
                let (mut cpu, mut bus) = fixture(
                    prefix,
                    opcode,
                    [0x12, 1],
                    [1, 0x8000, 0x7000, 0xffff],
                    [0x80, 0xf8],
                );
                bus.bytes[0x7000] = 0x34;
                let mut state = BusState::default();
                for &(clock, access) in &clocks[..count] {
                    phase(&mut cpu, &mut bus, &mut state, clock, false, access);
                }
                cpu.suspend();
                let saved = (cpu.clone(), bus.clone(), state);
                assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
                assert_eq!((cpu.clone(), bus.clone(), state), saved);
                cpu.resume();
                let mut copied = (cpu.clone(), bus.clone(), state);
                bus.bytes[0x7000] = 0x68;
                copied.1.bytes[0x7000] = 0x68;
                finish(&mut cpu, &mut bus, &mut state, &clocks, count);
                finish(&mut copied.0, &mut copied.1, &mut copied.2, &clocks, count);
                assert_eq!((cpu, bus, state), copied);
                let (mut cpu, mut bus, _) = saved;
                cpu.reset();
                bus.clear_trace();
                bus.bytes[0x100] = 0;
                let memory = bus.bytes.clone();
                phase(&mut cpu, &mut bus, &mut BusState::default(), 4, true, true);
                assert_eq!(bus.bytes, memory);
                assert_eq!(cpu.regs().pc, 0x101);
                assert_eq!(cpu.regs().sp, 0x9876);
            }
        }
    }
}

pub fn ed_internal_interrupt_boundaries() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            let clocks = phases(prefix, opcode);
            for count in 1..clocks.len() {
                for (iff1, iff2) in [(false, false), (false, true), (true, false), (true, true)] {
                    for nmi in [false, true] {
                        let (mut cpu, mut bus) = fixture(
                            prefix,
                            opcode,
                            [0x12, 1],
                            [1, 0x8000, 0x7000, 0xffff],
                            [0x80, 0xf8],
                        );
                        cpu.finish_guest_call(iff1, iff2, 0);
                        cpu.resume();
                        let mut state = BusState::default();
                        for &(clock, access) in &clocks[..count] {
                            phase(&mut cpu, &mut bus, &mut state, clock, false, access);
                        }
                        let polls = bus.polls.get();
                        bus.nmi = nmi;
                        bus.irq = !nmi;
                        finish(&mut cpu, &mut bus, &mut state, &clocks, count);
                        assert_eq!(cpu.interrupts_enabled(), iff1);
                        assert_eq!(cpu.saved_interrupts_enabled(), iff2);
                        assert_eq!(bus.polls.get(), polls);
                        bus.clear_trace();
                        cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                        assert_eq!(cpu.last_step_was_interrupt(), nmi || iff1);
                    }
                }
            }
            let (mut cpu, mut bus) = fixture(
                prefix,
                opcode,
                [0x12, 1],
                [1, 0x8000, 0x7000, 0xffff],
                [0x80, 0xf8],
            );
            bus.bytes.copy_within(0x4000..0x4006, 0x4001);
            bus.bytes[0x4000] = 0xfb;
            cpu.step_with_bus(&mut bus).unwrap();
            bus.clear_trace();
            bus.irq = true;
            let mut state = BusState::default();
            for (i, &(clock, access)) in clocks.iter().enumerate() {
                let complete = i + 1 == clocks.len();
                phase(&mut cpu, &mut bus, &mut state, clock, complete, access);
                assert_eq!(cpu.interrupts_enabled(), complete);
                assert!(!cpu.last_step_was_interrupt());
            }
            bus.clear_trace();
            cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert!(cpu.last_step_was_interrupt());
        }
    }
}

pub fn ed_internal_special_register_refresh_and_iff2() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in [0x47, 0x4f, 0x57, 0x5f] {
            for iff2 in [false, true] {
                for high in [false, true] {
                    let special = if high { [0xa8, 0xf8] } else { [0, 0x78] };
                    let (mut cpu, bus) = fixture(
                        prefix,
                        opcode,
                        [if high { 0xa8 } else { 0 }, 0xd7],
                        [1, 0x8000, 0x7000, 0xffff],
                        special,
                    );
                    cpu.finish_guest_call(false, iff2, 0);
                    cpu.resume();
                    assert_eq!(cpu.regs().r, if high { 0xfe } else { 0x7e });
                    let mut expected = cpu.regs();
                    let pv = if iff2 { 4 } else { 0 };
                    match opcode {
                        0x47 => expected.i = expected.a,
                        0x4f => expected.r = expected.a,
                        0x57 => {
                            expected.a = special[0];
                            expected.f = if high { 0xa9 } else { 0x41 } | pv;
                        }
                        _ => {
                            expected.a = if high { 0x80 } else { 0 } + u8::from(prefix.is_some());
                            expected.f = if high {
                                0x81
                            } else if prefix.is_some() {
                                1
                            } else {
                                0x41
                            } | pv;
                        }
                    }
                    literal_case(cpu, bus, prefix, opcode, expected, None);
                }
            }
        }
    }
}

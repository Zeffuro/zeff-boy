#[path = "../multi_load/support.rs"]
mod support;

use support::{Access, Bus, phase, seeded};
use zeff_z80::{BusState, Cpu, PrefixState, Registers, ResetState};

const OPCODES: [u8; 9] = [0xc1, 0xd1, 0xe1, 0xf1, 0xc5, 0xd5, 0xe5, 0xf5, 0xe3];

fn schedule(opcode: u8) -> &'static [u32] {
    match opcode {
        0xc5 | 0xd5 | 0xe5 | 0xf5 => &[4, 1, 3, 3],
        0xe3 => &[4, 3, 4, 3, 5],
        _ => &[4, 3, 3],
    }
}

fn step(cpu: &mut Cpu, bus: &mut Bus, state: &mut BusState, clocks: u32, complete: bool) {
    if clocks == 1 {
        let trace = bus.trace.borrow().clone();
        let result = cpu.step_bus_segment(bus, state).unwrap();
        assert_eq!(result.instruction.cycles, 1);
        assert!(!result.complete);
        assert!(state.pending());
        assert_eq!(*bus.trace.borrow(), trace);
    } else {
        phase(cpu, bus, state, clocks, complete);
    }
}

fn finish(cpu: &mut Cpu, bus: &mut Bus, state: &mut BusState, opcode: u8, from: usize) {
    let phases = schedule(opcode);
    for (i, &clocks) in phases.iter().enumerate().skip(from) {
        step(cpu, bus, state, clocks, i + 1 == phases.len());
    }
}

fn fixture(prefix: Option<u8>, opcode: u8, sp: u16, flags: u8) -> (Cpu, Bus) {
    let (mut cpu, mut bus) = seeded(0x9c);
    let pc = usize::from(cpu.regs().pc);
    bus.bytes[0x9876] = flags;
    bus.bytes[0x9877] = 0x9c;
    let [low, high] = sp.to_le_bytes();
    bus.bytes[pc..pc + 4].copy_from_slice(&[0xf1, 0x31, low, high]);
    cpu.step_with_bus(&mut bus).unwrap();
    bus.clear_trace();
    cpu.step_with_bus(&mut bus).unwrap();
    bus.clear_trace();
    let pc = usize::from(cpu.regs().pc);
    if let Some(prefix) = prefix {
        bus.bytes[pc..pc + 2].copy_from_slice(&[prefix, opcode]);
    } else {
        bus.bytes[pc] = opcode;
    }
    bus.bytes[usize::from(sp)] = flags;
    bus.bytes[usize::from(sp.wrapping_add(1))] = 0xa7;
    (cpu, bus)
}

fn selected(regs: Registers, prefix: Option<u8>, opcode: u8) -> u16 {
    match ((opcode >> 4) & 3, prefix) {
        (0, _) => regs.bc(),
        (1, _) => regs.de(),
        (2, Some(0xdd)) => regs.ix,
        (2, Some(0xfd)) => regs.iy,
        (2, _) => regs.hl(),
        _ => regs.af(),
    }
}

fn set_selected(regs: &mut Registers, prefix: Option<u8>, opcode: u8, value: u16) {
    let [high, low] = value.to_be_bytes();
    match ((opcode >> 4) & 3, prefix) {
        (0, _) => {
            regs.b = high;
            regs.c = low;
        }
        (1, _) => {
            regs.d = high;
            regs.e = low;
        }
        (2, Some(0xdd)) => regs.ix = value,
        (2, Some(0xfd)) => regs.iy = value,
        (2, _) => {
            regs.h = high;
            regs.l = low;
        }
        _ => {
            regs.a = high;
            regs.f = low;
        }
    }
}

pub fn all_stack_forms_match_literal_effects_and_steppers() {
    let mut forms = 0;
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            forms += 1;
            for flags in [0, 1, 0x28, 0xd6, 0xff] {
                for sp in [0, 1, 0x7000, 0xffff] {
                    let (mut cpu, mut bus) = fixture(prefix, opcode, sp, flags);
                    let mut expected_regs = cpu.regs();
                    let mut expected_memory = bus.bytes.clone();
                    let mut accesses = Vec::new();
                    let pc = expected_regs.pc;
                    if let Some(prefix) = prefix {
                        accesses.push(Some(Access::Read(pc, prefix)));
                    }
                    accesses.push(Some(Access::Read(pc + u16::from(prefix.is_some()), opcode)));
                    expected_regs.pc += 1 + u16::from(prefix.is_some());
                    expected_regs.r += 1 + u8::from(prefix.is_some());
                    let value = selected(expected_regs, prefix, opcode);
                    let [low, high] = value.to_le_bytes();
                    if opcode & 7 == 5 {
                        accesses.push(Some(Access::Write(sp.wrapping_sub(1), high)));
                        accesses.push(Some(Access::Write(sp.wrapping_sub(2), low)));
                        expected_memory[usize::from(sp.wrapping_sub(1))] = high;
                        expected_memory[usize::from(sp.wrapping_sub(2))] = low;
                        expected_regs.sp = sp.wrapping_sub(2);
                    } else {
                        accesses.push(Some(Access::Read(sp, flags)));
                        accesses.push(Some(Access::Read(sp.wrapping_add(1), 0xa7)));
                        set_selected(
                            &mut expected_regs,
                            prefix,
                            opcode,
                            u16::from_le_bytes([flags, 0xa7]),
                        );
                        if opcode == 0xe3 {
                            accesses.push(Some(Access::Write(sp.wrapping_add(1), high)));
                            accesses.push(Some(Access::Write(sp, low)));
                            expected_memory[usize::from(sp.wrapping_add(1))] = high;
                            expected_memory[usize::from(sp)] = low;
                        } else {
                            expected_regs.sp = sp.wrapping_add(2);
                        }
                    }
                    let mut legacy = (cpu.clone(), bus.clone());
                    let mut bounded = legacy.clone();
                    let total = legacy.0.step_with_bus(&mut legacy.1).unwrap().cycles;
                    let mut prefix_state = PrefixState::default();
                    let mut bounded_clocks = 0;
                    loop {
                        let result = bounded
                            .0
                            .step_bounded(&mut bounded.1, &mut prefix_state)
                            .unwrap();
                        bounded_clocks += result.instruction.cycles;
                        if result.complete {
                            break;
                        }
                    }
                    assert_eq!(bounded, legacy);
                    assert_eq!(bounded_clocks, total);
                    let initial_cycles = cpu.cycles();
                    let mut state = BusState::default();
                    if prefix.is_some() {
                        step(&mut cpu, &mut bus, &mut state, 4, false);
                    }
                    for (i, &clocks) in schedule(opcode).iter().enumerate() {
                        step(
                            &mut cpu,
                            &mut bus,
                            &mut state,
                            clocks,
                            i + 1 == schedule(opcode).len(),
                        );
                        let expected_sp = match opcode {
                            0xe3 => sp,
                            _ if opcode & 7 == 5 => sp.wrapping_sub(i.min(2) as u16),
                            _ => sp.wrapping_add(i as u16),
                        };
                        assert_eq!(cpu.regs().sp, expected_sp);
                        assert_eq!(state.prefix_pending(), prefix.is_some() && state.pending());
                    }
                    assert_eq!(
                        total,
                        schedule(opcode).iter().sum::<u32>() + 4 * u32::from(prefix.is_some())
                    );
                    assert_eq!(cpu.cycles() - initial_cycles, u64::from(total));
                    assert_eq!(cpu.regs(), expected_regs, "{prefix:?} {opcode:02x}");
                    assert_eq!(bus.bytes, expected_memory);
                    assert_eq!(bus.trace.borrow().count, accesses.len());
                    assert_eq!(
                        &bus.trace.borrow().events[..accesses.len()],
                        accesses.as_slice()
                    );
                    assert_eq!((cpu, bus), legacy);
                }
            }
        }
    }
    assert_eq!(forms, 27);
}

pub fn stack_live_reads_and_latched_writes() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            let (mut cpu, mut bus) = fixture(prefix, opcode, 0xffff, 0xff);
            let pc = usize::from(cpu.regs().pc);
            let source = selected(cpu.regs(), prefix, opcode);
            let mut state = BusState::default();
            if prefix.is_some() {
                step(&mut cpu, &mut bus, &mut state, 4, false);
            }
            step(&mut cpu, &mut bus, &mut state, 4, false);
            bus.bytes[pc + usize::from(prefix.is_some())] = 0;
            if opcode & 7 == 5 {
                step(&mut cpu, &mut bus, &mut state, 1, false);
                step(&mut cpu, &mut bus, &mut state, 3, false);
                bus.bytes[0xfffe] = 0x19;
                bus.bytes[0xfffd] = 0x28;
                step(&mut cpu, &mut bus, &mut state, 3, true);
                assert_eq!(bus.bytes[0xfffe], 0x19);
                assert_eq!(bus.bytes[0xfffd], source as u8);
                assert_eq!(selected(cpu.regs(), prefix, opcode), source);
            } else {
                bus.bytes[0xffff] = 0x53;
                step(&mut cpu, &mut bus, &mut state, 3, false);
                assert_eq!(selected(cpu.regs(), prefix, opcode), source);
                bus.bytes[0xffff] = 0x62;
                bus.bytes[0] = 0x84;
                step(
                    &mut cpu,
                    &mut bus,
                    &mut state,
                    if opcode == 0xe3 { 4 } else { 3 },
                    opcode != 0xe3,
                );
                if opcode == 0xe3 {
                    assert_eq!(selected(cpu.regs(), prefix, opcode), source);
                    bus.bytes[0] = 0x95;
                    step(&mut cpu, &mut bus, &mut state, 3, false);
                    assert_eq!(bus.bytes[0], (source >> 8) as u8);
                    assert_eq!(selected(cpu.regs(), prefix, opcode), source);
                    bus.bytes[0] = 0xa6;
                    step(&mut cpu, &mut bus, &mut state, 5, true);
                    assert_eq!(bus.bytes[0], 0xa6);
                    assert_eq!(bus.bytes[0xffff], source as u8);
                }
                assert_eq!(selected(cpu.regs(), prefix, opcode), 0x8453);
            }
        }
    }
}

pub fn stack_pending_phases_clone_suspend_and_reset() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            let mut clocks = Vec::new();
            if prefix.is_some() {
                clocks.push(4);
            }
            clocks.extend_from_slice(schedule(opcode));
            for count in 1..clocks.len() {
                let (mut cpu, mut bus) = fixture(prefix, opcode, 0xffff, 0xff);
                let mut state = BusState::default();
                for &clocks in &clocks[..count] {
                    step(&mut cpu, &mut bus, &mut state, clocks, false);
                }
                cpu.suspend();
                let saved = (cpu.clone(), bus.clone(), state);
                assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
                assert_eq!((cpu.clone(), bus.clone(), state), saved);
                cpu.resume();
                let mut copied = (cpu.clone(), bus.clone(), state);
                for (i, &clock) in clocks.iter().enumerate().skip(count) {
                    step(&mut cpu, &mut bus, &mut state, clock, i + 1 == clocks.len());
                    step(
                        &mut copied.0,
                        &mut copied.1,
                        &mut copied.2,
                        clock,
                        i + 1 == clocks.len(),
                    );
                }
                assert_eq!((cpu, bus, state), copied);
                let (mut reset_cpu, mut reset_bus, _) = saved;
                reset_cpu.reset();
                reset_bus.clear_trace();
                reset_bus.bytes[0x100] = 0;
                let memory = reset_bus.bytes.clone();
                step(
                    &mut reset_cpu,
                    &mut reset_bus,
                    &mut BusState::default(),
                    4,
                    true,
                );
                assert_eq!(reset_bus.bytes, memory);
                assert_eq!(reset_cpu.regs().pc, 0x101);
                assert_eq!(reset_cpu.regs().sp, 0x9876);
            }
        }
    }
}

pub fn stack_prefixes_and_interrupt_boundaries() {
    for prefix in [0xdd, 0xfd] {
        for opcode in OPCODES {
            for count in 1..schedule(opcode).len() {
                for nmi in [false, true] {
                    let (mut cpu, mut bus) = fixture(Some(prefix), opcode, 0x7000, 0xff);
                    let pc = usize::from(cpu.regs().pc);
                    let other = if prefix == 0xdd { 0xfd } else { 0xdd };
                    bus.bytes[pc..pc + 5].copy_from_slice(&[0xfb, other, prefix, opcode, 0]);
                    cpu.step_with_bus(&mut bus).unwrap();
                    bus.clear_trace();
                    let mut legacy = (cpu.clone(), bus.clone());
                    let mut state = BusState::default();
                    for _ in 0..2 {
                        step(&mut cpu, &mut bus, &mut state, 4, false);
                    }
                    for &clocks in &schedule(opcode)[..count] {
                        step(&mut cpu, &mut bus, &mut state, clocks, false);
                    }
                    let polls = bus.polls.get();
                    bus.nmi = nmi;
                    bus.irq = !nmi;
                    assert!(!cpu.interrupts_enabled());
                    finish(&mut cpu, &mut bus, &mut state, opcode, count);
                    assert!(cpu.interrupts_enabled());
                    assert_eq!(bus.polls.get(), polls);
                    assert_eq!(cpu.last_m1_fetch_count(), 3);
                    assert!(!cpu.last_step_was_interrupt());
                    legacy.0.step_with_bus(&mut legacy.1).unwrap();
                    assert_eq!(cpu, legacy.0);
                    bus.irq = false;
                    bus.nmi = false;
                    assert_eq!(bus, legacy.1);
                    bus.clear_trace();
                    bus.nmi = nmi;
                    bus.irq = !nmi;
                    for (index, clocks) in [if nmi { 5 } else { 7 }, 3, 3].into_iter().enumerate() {
                        let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                        assert_eq!(step.instruction.cycles, clocks);
                        assert_eq!(step.complete, index == 2);
                    }
                    assert!(cpu.last_step_was_interrupt());
                    assert_eq!(bus.acknowledgements, u32::from(nmi));
                }
            }
        }
    }
}

pub fn stack_pc_wrap_and_instruction_aliases() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            let mut cpu = Cpu::new_with_reset(ResetState::new(0xffff, 0xffff));
            let mut bus = Bus::new();
            bus.bytes[0xffff] = prefix.unwrap_or(opcode);
            bus.bytes[0] = if prefix.is_some() { opcode } else { 0x93 };
            let result = u16::from_le_bytes([bus.bytes[0xffff], bus.bytes[0]]);
            let mut legacy = (cpu.clone(), bus.clone());
            legacy.0.step_with_bus(&mut legacy.1).unwrap();
            let mut state = BusState::default();
            if prefix.is_some() {
                step(&mut cpu, &mut bus, &mut state, 4, false);
            }
            finish(&mut cpu, &mut bus, &mut state, opcode, 0);
            assert_eq!(cpu.regs().pc, u16::from(prefix.is_some()));
            assert_eq!(cpu.regs().r, 1 + u8::from(prefix.is_some()));
            if opcode & 7 != 5 {
                assert_eq!(selected(cpu.regs(), prefix, opcode), result);
            }
            assert_eq!((cpu, bus), legacy);
        }
    }
}

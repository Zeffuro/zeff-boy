#[path = "../multi_load/support.rs"]
mod support;

use support::{Access, Bus, phase, seeded};
use zeff_z80::{BusState, Cpu, PrefixState};

const OPCODES: [u8; 26] = [
    0xcd, 0xc4, 0xcc, 0xd4, 0xdc, 0xe4, 0xec, 0xf4, 0xfc, 0xc9, 0xc0, 0xc8, 0xd0, 0xd8, 0xe0, 0xe8,
    0xf0, 0xf8, 0xc7, 0xcf, 0xd7, 0xdf, 0xe7, 0xef, 0xf7, 0xff,
];

fn call(opcode: u8) -> bool {
    opcode == 0xcd || opcode & 0xc7 == 0xc4
}

fn ret(opcode: u8) -> bool {
    opcode == 0xc9 || opcode & 0xc7 == 0xc0
}

fn taken(opcode: u8, flags: u8) -> bool {
    if opcode == 0xcd || opcode == 0xc9 || opcode & 7 == 7 {
        return true;
    }
    let mask = [0x40, 0x40, 1, 1, 4, 4, 0x80, 0x80][usize::from((opcode >> 3) & 7)];
    (flags & mask != 0) == (opcode & 8 != 0)
}

fn schedule(opcode: u8, flags: u8) -> &'static [u32] {
    if call(opcode) {
        if taken(opcode, flags) {
            &[4, 3, 3, 1, 3, 3]
        } else {
            &[4, 3, 3]
        }
    } else if opcode == 0xc9 {
        &[4, 3, 3]
    } else if ret(opcode) {
        if taken(opcode, flags) {
            &[4, 1, 3, 3]
        } else {
            &[4, 1]
        }
    } else {
        &[4, 1, 3, 3]
    }
}

fn step(cpu: &mut Cpu, bus: &mut Bus, state: &mut BusState, clocks: u32, complete: bool) {
    if clocks != 1 {
        phase(cpu, bus, state, clocks, complete);
        return;
    }
    let count = bus.trace.borrow().count;
    let result = cpu.step_bus_segment(bus, state).unwrap();
    assert_eq!(result.instruction.cycles, clocks);
    assert_eq!(result.complete, complete);
    assert_eq!(state.pending(), !complete);
    assert_eq!(bus.trace.borrow().count, count + usize::from(clocks != 1));
}

fn fixture(prefix: Option<u8>, opcode: u8, sp: u16, flags: u8, pc: u16) -> (Cpu, Bus) {
    let (mut cpu, mut bus) = seeded(0x9c);
    let setup = usize::from(cpu.regs().pc);
    bus.bytes[0x9876] = flags;
    bus.bytes[0x9877] = 0x9c;
    let [low, high] = sp.to_le_bytes();
    let [pc_low, pc_high] = pc.to_le_bytes();
    bus.bytes[setup..setup + 7].copy_from_slice(&[0xf1, 0x31, low, high, 0xc3, pc_low, pc_high]);
    for _ in 0..3 {
        cpu.step_with_bus(&mut bus).unwrap();
        bus.clear_trace();
    }
    bus.bytes[usize::from(sp)] = 0x53;
    bus.bytes[usize::from(sp.wrapping_add(1))] = 0xa7;
    let mut terminal = pc;
    if let Some(prefix) = prefix {
        bus.bytes[usize::from(terminal)] = prefix;
        terminal = terminal.wrapping_add(1);
    }
    bus.bytes[usize::from(terminal)] = opcode;
    if call(opcode) {
        bus.bytes[usize::from(terminal.wrapping_add(1))] = 0x34;
        bus.bytes[usize::from(terminal.wrapping_add(2))] = 0x12;
    }
    (cpu, bus)
}

fn finish(cpu: &mut Cpu, bus: &mut Bus, state: &mut BusState, opcode: u8, flags: u8, from: usize) {
    let phases = schedule(opcode, flags);
    for (i, &clocks) in phases.iter().enumerate().skip(from) {
        step(cpu, bus, state, clocks, i + 1 == phases.len());
    }
}

fn literal_case(prefix: Option<u8>, opcode: u8, sp: u16, flags: u8, pc: u16) {
    let (mut cpu, mut bus) = fixture(prefix, opcode, sp, flags, pc);
    let mut expected_regs = cpu.regs();
    let mut expected_memory = bus.bytes.clone();
    let mut accesses = Vec::new();
    let terminal = pc.wrapping_add(u16::from(prefix.is_some()));
    if let Some(prefix) = prefix {
        accesses.push(Some(Access::Read(pc, prefix)));
    }
    accesses.push(Some(Access::Read(terminal, opcode)));
    let sequential = terminal.wrapping_add(if call(opcode) { 3 } else { 1 });
    expected_regs.pc = sequential;
    expected_regs.r += 1 + u8::from(prefix.is_some());
    if call(opcode) {
        accesses.push(Some(Access::Read(terminal.wrapping_add(1), 0x34)));
        accesses.push(Some(Access::Read(terminal.wrapping_add(2), 0x12)));
    }
    if taken(opcode, flags) {
        if ret(opcode) {
            let low = expected_memory[usize::from(sp)];
            let high = expected_memory[usize::from(sp.wrapping_add(1))];
            accesses.push(Some(Access::Read(sp, low)));
            accesses.push(Some(Access::Read(sp.wrapping_add(1), high)));
            expected_regs.sp = sp.wrapping_add(2);
            expected_regs.pc = u16::from_le_bytes([low, high]);
        } else {
            let [low, high] = sequential.to_le_bytes();
            accesses.push(Some(Access::Write(sp.wrapping_sub(1), high)));
            accesses.push(Some(Access::Write(sp.wrapping_sub(2), low)));
            expected_memory[usize::from(sp.wrapping_sub(1))] = high;
            expected_memory[usize::from(sp.wrapping_sub(2))] = low;
            expected_regs.sp = sp.wrapping_sub(2);
            expected_regs.pc = if call(opcode) {
                0x1234
            } else {
                u16::from(opcode & 0x38)
            };
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
    let phases = schedule(opcode, flags);
    for (i, &clocks) in phases.iter().enumerate() {
        step(
            &mut cpu,
            &mut bus,
            &mut state,
            clocks,
            i + 1 == phases.len(),
        );
        let expected_sp = if !taken(opcode, flags) {
            sp
        } else if ret(opcode) {
            sp.wrapping_add(if opcode == 0xc9 {
                i.min(2)
            } else {
                i.saturating_sub(1).min(2)
            } as u16)
        } else {
            sp.wrapping_sub(if call(opcode) {
                i.saturating_sub(2).min(2)
            } else {
                i.min(2)
            } as u16)
        };
        assert_eq!(
            cpu.regs().sp,
            expected_sp,
            "{prefix:?} {opcode:02x} phase{i}"
        );
        let phase_pc = if i + 1 == phases.len() {
            expected_regs.pc
        } else if call(opcode) {
            terminal.wrapping_add(1 + i.min(2) as u16)
        } else {
            sequential
        };
        assert_eq!(cpu.regs().pc, phase_pc);
        assert_eq!(state.prefix_pending(), prefix.is_some() && state.pending());
    }
    assert_eq!(
        total,
        phases.iter().sum::<u32>() + 4 * u32::from(prefix.is_some())
    );
    assert_eq!(cpu.cycles() - initial_cycles, u64::from(total));
    assert_eq!(
        cpu.regs(),
        expected_regs,
        "{prefix:?} {opcode:02x} {flags:02x} {sp:04x}"
    );
    assert_eq!(bus.bytes, expected_memory);
    assert_eq!(bus.trace.borrow().count, accesses.len());
    assert_eq!(
        &bus.trace.borrow().events[..accesses.len()],
        accesses.as_slice()
    );
    assert_eq!((cpu, bus), legacy);
}

pub fn all_control_stack_forms_match_literal_effects_and_steppers() {
    let mut forms = 0;
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            forms += 1;
            for flags in [0, 0xff] {
                for sp in [0, 1, 0x7000, 0xffff] {
                    literal_case(prefix, opcode, sp, flags, 0x4000);
                }
            }
        }
    }
    assert_eq!(forms, 78);
}

pub fn control_stack_live_reads_and_latched_targets() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            for flags in [0, 0xff] {
                let (mut cpu, mut bus) = fixture(prefix, opcode, 0xffff, flags, 0x4000);
                let terminal = 0x4000 + usize::from(prefix.is_some());
                let mut state = BusState::default();
                if prefix.is_some() {
                    step(&mut cpu, &mut bus, &mut state, 4, false);
                }
                step(&mut cpu, &mut bus, &mut state, 4, false);
                bus.bytes[terminal] = 0;
                if call(opcode) {
                    step(&mut cpu, &mut bus, &mut state, 3, false);
                    bus.bytes[terminal + 1] = 0x95;
                    bus.bytes[terminal + 2] = 0x84;
                    step(&mut cpu, &mut bus, &mut state, 3, !taken(opcode, flags));
                    let sequential = terminal as u16 + 3;
                    assert_eq!(cpu.regs().pc, sequential);
                    bus.bytes[terminal + 2] = 0xa6;
                    if taken(opcode, flags) {
                        step(&mut cpu, &mut bus, &mut state, 1, false);
                        step(&mut cpu, &mut bus, &mut state, 3, false);
                        assert_eq!(cpu.regs().pc, sequential);
                        assert_eq!(bus.bytes[0xfffe], (sequential >> 8) as u8);
                        bus.bytes[0xfffe] = 0x19;
                        step(&mut cpu, &mut bus, &mut state, 3, true);
                        assert_eq!(cpu.regs().pc, 0x8434);
                        assert_eq!(bus.bytes[0xfffe], 0x19);
                        assert_eq!(bus.bytes[0xfffd], sequential as u8);
                    }
                } else if ret(opcode) {
                    if opcode != 0xc9 {
                        step(&mut cpu, &mut bus, &mut state, 1, !taken(opcode, flags));
                    }
                    if taken(opcode, flags) {
                        bus.bytes[0xffff] = 0x62;
                        step(&mut cpu, &mut bus, &mut state, 3, false);
                        assert_eq!(cpu.regs().pc, terminal as u16 + 1);
                        assert_eq!(cpu.regs().sp, 0);
                        bus.bytes[0xffff] = 0x73;
                        bus.bytes[0] = 0x84;
                        step(&mut cpu, &mut bus, &mut state, 3, true);
                        assert_eq!(cpu.regs().pc, 0x8462);
                        assert_eq!(cpu.regs().sp, 1);
                    }
                } else {
                    let sequential = terminal as u16 + 1;
                    step(&mut cpu, &mut bus, &mut state, 1, false);
                    step(&mut cpu, &mut bus, &mut state, 3, false);
                    assert_eq!(cpu.regs().pc, sequential);
                    bus.bytes[0xfffe] = 0x19;
                    step(&mut cpu, &mut bus, &mut state, 3, true);
                    assert_eq!(cpu.regs().pc, u16::from(opcode & 0x38));
                    assert_eq!(bus.bytes[0xfffe], 0x19);
                    assert_eq!(bus.bytes[0xfffd], sequential as u8);
                }
                assert!(!state.pending());
                assert_eq!(cpu.regs().f, flags);
            }
        }
    }
}

pub fn control_stack_pending_phases_clone_suspend_and_reset() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            for flags in [0, 0xff] {
                let mut clocks = Vec::new();
                if prefix.is_some() {
                    clocks.push(4);
                }
                clocks.extend_from_slice(schedule(opcode, flags));
                for count in 1..clocks.len() {
                    let (mut cpu, mut bus) = fixture(prefix, opcode, 0xffff, flags, 0x4000);
                    let mut state = BusState::default();
                    for &clock in &clocks[..count] {
                        step(&mut cpu, &mut bus, &mut state, clock, false);
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
}

pub fn control_stack_prefixes_and_interrupt_boundaries() {
    for prefix in [0xdd, 0xfd] {
        for opcode in OPCODES {
            for flags in [0, 0xff] {
                for count in 0..schedule(opcode, flags).len() {
                    for nmi in [false, true] {
                        let (mut cpu, mut bus) =
                            fixture(Some(prefix), opcode, 0x7000, flags, 0x4000);
                        let other = if prefix == 0xdd { 0xfd } else { 0xdd };
                        bus.bytes[0x4000..0x4006]
                            .copy_from_slice(&[0xfb, other, prefix, opcode, 0x34, 0x12]);
                        cpu.step_with_bus(&mut bus).unwrap();
                        bus.clear_trace();
                        let mut legacy = (cpu.clone(), bus.clone());
                        let mut state = BusState::default();
                        for _ in 0..2 {
                            step(&mut cpu, &mut bus, &mut state, 4, false);
                        }
                        for &clock in &schedule(opcode, flags)[..count] {
                            step(&mut cpu, &mut bus, &mut state, clock, false);
                        }
                        let polls = bus.polls.get();
                        bus.nmi = nmi;
                        bus.irq = !nmi;
                        assert!(!cpu.interrupts_enabled());
                        finish(&mut cpu, &mut bus, &mut state, opcode, flags, count);
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
                        for (index, clocks) in
                            [if nmi { 5 } else { 7 }, 3, 3].into_iter().enumerate()
                        {
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
}

pub fn control_stack_pc_wrap_and_instruction_aliases() {
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            for flags in [0, 0xff] {
                for pc in [0x4000u16, 0xffff] {
                    let terminal = pc.wrapping_add(u16::from(prefix.is_some()));
                    let alias = if call(opcode) {
                        terminal.wrapping_add(3)
                    } else {
                        terminal
                    };
                    for sp in [0, 1, 0xffff, alias] {
                        literal_case(prefix, opcode, sp, flags, pc);
                    }
                }
            }
        }
    }
}

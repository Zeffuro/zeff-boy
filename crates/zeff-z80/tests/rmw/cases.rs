#[allow(dead_code)]
#[path = "../multi_load/support.rs"]
mod support;
use support::{Access, Bus, seeded};
use zeff_z80::{BusState, Cpu, CpuState, PrefixState};

fn step(cpu: &mut Cpu, bus: &mut Bus, state: &mut BusState, clocks: u32, complete: bool) {
    let result = cpu.step_bus_segment(bus, state).unwrap();
    assert_eq!(result.instruction.cycles, clocks);
    assert_eq!(result.complete, complete);
    assert_eq!(state.pending(), !complete);
}

pub fn all_cb_and_memory_inc_dec_match_legacy() {
    for terminal in 0..258u16 {
        for value in [0, 1, 0x7f, 0x80, 0x81, 0xff] {
            let (mut cpu, mut bus) = seeded(value);
            let pc = usize::from(cpu.regs().pc);
            let cb = terminal < 256;
            let opcode = if cb {
                terminal as u8
            } else if terminal == 256 {
                0x34
            } else {
                0x35
            };
            bus.bytes[pc] = if cb { 0xcb } else { opcode };
            bus.bytes[pc + 1] = opcode;
            let address = usize::from(cpu.regs().hl());
            bus.bytes[address] = value;
            let mut legacy = cpu.clone();
            let mut legacy_bus = bus.clone();
            let mut bounded = cpu.clone();
            let mut bounded_bus = bus.clone();
            let mut prefix = PrefixState::default();
            let mut state = BusState::default();
            let initial = cpu.cycles();
            step(&mut cpu, &mut bus, &mut state, 4, false);
            if cb {
                step(&mut cpu, &mut bus, &mut state, 4, opcode & 7 != 6);
            }
            if !cb || opcode & 7 == 6 {
                let bit = cb && opcode >> 6 == 1;
                assert_eq!(bus.trace.borrow().count, if cb { 2 } else { 1 });
                assert_eq!(bus.bytes[address], value);
                step(&mut cpu, &mut bus, &mut state, 4, bit);
                if !bit {
                    step(&mut cpu, &mut bus, &mut state, 3, true);
                }
            }
            let whole = legacy.step_with_bus(&mut legacy_bus).unwrap();
            bounded.step_bounded(&mut bounded_bus, &mut prefix).unwrap();
            assert_eq!(cpu.cycles() - initial, u64::from(whole.cycles));
            assert_eq!(cpu, legacy);
            assert_eq!(bus, legacy_bus);
            assert_eq!(cpu, bounded);
            assert_eq!(bus, bounded_bus);
            assert_eq!(
                bus.trace.borrow().count,
                if !cb {
                    3
                } else if opcode & 7 != 6 {
                    2
                } else if opcode >> 6 == 1 {
                    3
                } else {
                    4
                }
            );
        }
    }
}

pub fn read_value_and_flags_stay_latched() {
    for (program, value, result, flags) in [
        (&[0x34][..], 0x7f, 0x80, 0x95),
        (&[0x35][..], 0x80, 0x7f, 0x3f),
        (&[0xcb, 6][..], 0x81, 3, 5),
        (&[0xcb, 0x86][..], 0x81, 0x80, 1),
    ] {
        let (mut cpu, mut bus) = seeded(0);
        let pc = usize::from(cpu.regs().pc);
        bus.bytes[pc..pc + program.len()].copy_from_slice(program);
        let address = usize::from(cpu.regs().hl());
        bus.bytes[address] = value;
        let mut state = BusState::default();
        step(&mut cpu, &mut bus, &mut state, 4, false);
        if program.len() == 2 {
            step(&mut cpu, &mut bus, &mut state, 4, false);
        }
        step(&mut cpu, &mut bus, &mut state, 4, false);
        assert_eq!(cpu.regs().f, 1);
        assert_eq!(bus.bytes[address], value);
        bus.bytes[address] = 0x55;
        let mut copied = (cpu.clone(), bus.clone(), state);
        step(&mut cpu, &mut bus, &mut state, 3, true);
        step(&mut copied.0, &mut copied.1, &mut copied.2, 3, true);
        assert_eq!((cpu.clone(), bus.clone(), state), copied);
        assert_eq!(bus.bytes[address], result);
        assert_eq!(cpu.regs().f, flags);
        assert_eq!(
            bus.trace.borrow().events[bus.trace.borrow().count - 1],
            Some(Access::Write(address as u16, result))
        );
    }
    for (carry, result, flags) in [(true, 0xc0, 0x85), (false, 0x40, 1)] {
        let (mut cpu, mut bus) = seeded(0);
        if !carry {
            let pc = usize::from(cpu.regs().pc);
            bus.bytes[pc] = 0x3f;
            cpu.step_with_bus(&mut bus).unwrap();
            bus.clear_trace();
        }
        let pc = usize::from(cpu.regs().pc);
        bus.bytes[pc..pc + 2].copy_from_slice(&[0xcb, 0x1e]);
        let address = usize::from(cpu.regs().hl());
        bus.bytes[address] = 0x81;
        let mut state = BusState::default();
        step(&mut cpu, &mut bus, &mut state, 4, false);
        step(&mut cpu, &mut bus, &mut state, 4, false);
        step(&mut cpu, &mut bus, &mut state, 4, false);
        bus.bytes[address] = 0;
        step(&mut cpu, &mut bus, &mut state, 3, true);
        assert_eq!(bus.bytes[address], result);
        assert_eq!(cpu.regs().f, flags);
    }
}

pub fn cb_terminal_fetch_is_live() {
    let (mut cpu, mut bus) = seeded(0);
    let pc = usize::from(cpu.regs().pc);
    bus.bytes[pc..pc + 2].copy_from_slice(&[0xcb, 6]);
    let mut state = BusState::default();
    step(&mut cpu, &mut bus, &mut state, 4, false);
    bus.bytes[pc] = 0;
    bus.bytes[pc + 1] = 0xc0;
    step(&mut cpu, &mut bus, &mut state, 4, true);
    assert_eq!(cpu.regs().b, 0x11);
    assert_eq!(cpu.last_m1_fetch_count(), 2);
    assert_eq!(
        bus.trace.borrow().events[1],
        Some(Access::Read((pc + 1) as u16, 0xc0))
    );
}

pub fn pending_write_defers_interrupts_and_reset() {
    let (mut cpu, mut bus) = seeded(0);
    let pc = usize::from(cpu.regs().pc);
    bus.bytes[pc..pc + 3].copy_from_slice(&[0xfb, 0x34, 0]);
    let mut state = BusState::default();
    step(&mut cpu, &mut bus, &mut state, 4, true);
    step(&mut cpu, &mut bus, &mut state, 4, false);
    step(&mut cpu, &mut bus, &mut state, 4, false);
    bus.nmi = true;
    bus.irq = true;
    let polls = bus.polls.get();
    cpu.suspend();
    let saved = (cpu.clone(), bus.clone(), state);
    assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
    assert_eq!((cpu.clone(), bus.clone(), state), saved);
    cpu.resume();
    let mut reset = cpu.clone();
    reset.reset();
    let reset_state = BusState::default();
    assert!(!reset_state.pending());
    assert_eq!(reset.state(), CpuState::Running);
    step(&mut cpu, &mut bus, &mut state, 3, true);
    assert_eq!(bus.polls.get(), polls);
    assert!(bus.nmi);
    cpu.step_bus_segment(&mut bus, &mut state).unwrap();
    assert!(!bus.nmi);
}

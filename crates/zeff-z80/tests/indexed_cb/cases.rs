#[allow(dead_code)]
#[path = "../multi_load/support.rs"]
mod support;
use support::{Access, Bus, phase, seeded};
use zeff_z80::{BusState, Cpu, PrefixState, Registers, ResetState};

fn fixture(prefix: u8, opcode: u8, base: u16, displacement: u8, value: u8) -> (Cpu, Bus) {
    let (mut cpu, mut bus) = seeded(0x80);
    let pc = usize::from(cpu.regs().pc);
    let [low, high] = base.to_le_bytes();
    bus.bytes[pc..pc + 4].copy_from_slice(&[prefix, 0x21, low, high]);
    cpu.step_with_bus(&mut bus).unwrap();
    bus.clear_trace();
    let pc = usize::from(cpu.regs().pc);
    bus.bytes[pc..pc + 4].copy_from_slice(&[prefix, 0xcb, displacement, opcode]);
    let address = base.wrapping_add_signed(i16::from(displacement as i8));
    bus.bytes[usize::from(address)] = value;
    (cpu, bus)
}

fn schedule(opcode: u8) -> &'static [u32] {
    if (0x40..0x80).contains(&opcode) {
        &[4, 4, 3, 5, 4]
    } else {
        &[4, 4, 3, 5, 4, 3]
    }
}

fn finish(cpu: &mut Cpu, bus: &mut Bus, state: &mut BusState, opcode: u8, from: usize) {
    let phases = schedule(opcode);
    for (index, &clocks) in phases.iter().enumerate().skip(from) {
        phase(cpu, bus, state, clocks, index + 1 == phases.len());
    }
}

fn copy_register(regs: &mut Registers, register: u8, value: u8) {
    match register {
        0 => regs.b = value,
        1 => regs.c = value,
        2 => regs.d = value,
        3 => regs.e = value,
        4 => regs.h = value,
        5 => regs.l = value,
        6 => {}
        7 => regs.a = value,
        _ => unreachable!(),
    }
}

pub fn all_indexed_cb_forms_match_literal_effects_and_steppers() {
    let results = [
        [
            (0x03, 0x05),
            (0xc0, 0x85),
            (0x03, 0x05),
            (0xc0, 0x85),
            (0x02, 0x01),
            (0xc0, 0x85),
            (0x03, 0x05),
            (0x40, 0x01),
        ],
        [
            (0x81, 0x39),
            (0x81, 0x7d),
            (0x81, 0x7d),
            (0x81, 0x7d),
            (0x81, 0x7d),
            (0x81, 0x7d),
            (0x81, 0x7d),
            (0x81, 0xb9),
        ],
        [
            (0x80, 0x01),
            (0x81, 0x01),
            (0x81, 0x01),
            (0x81, 0x01),
            (0x81, 0x01),
            (0x81, 0x01),
            (0x81, 0x01),
            (0x01, 0x01),
        ],
        [
            (0x81, 0x01),
            (0x83, 0x01),
            (0x85, 0x01),
            (0x89, 0x01),
            (0x91, 0x01),
            (0xa1, 0x01),
            (0xc1, 0x01),
            (0x81, 0x01),
        ],
    ];
    let mut encodings = 0;
    for prefix in [0xdd, 0xfd] {
        for opcode in 0..=255u8 {
            encodings += 1;
            let (mut cpu, mut bus) = fixture(prefix, opcode, 0x2900, 0xff, 0x81);
            let pc = cpu.regs().pc;
            let cycles = cpu.cycles();
            let mut expected = cpu.regs();
            let (value, flags) = results[usize::from(opcode >> 6)][usize::from((opcode >> 3) & 7)];
            let bit = (0x40..0x80).contains(&opcode);
            if !bit {
                copy_register(&mut expected, opcode & 7, value);
            }
            expected.f = flags;
            expected.pc += 4;
            expected.r = (expected.r & 0x80) | (expected.r.wrapping_add(2) & 0x7f);
            let mut legacy = (cpu.clone(), bus.clone());
            let mut bounded = (cpu.clone(), bus.clone(), PrefixState::default());
            let mut state = BusState::default();
            finish(&mut cpu, &mut bus, &mut state, opcode, 0);
            assert_eq!(cpu.regs(), expected, "{prefix:02x} CB FF {opcode:02x}");
            assert_eq!(cpu.cycles() - cycles, if bit { 20 } else { 23 });
            assert_eq!(cpu.last_m1_fetch_count(), 2);
            assert_eq!(bus.bytes[0x28ff], value);
            let trace = bus.trace.borrow();
            assert_eq!(
                &trace.events[..5],
                &[
                    Some(Access::Read(pc, prefix)),
                    Some(Access::Read(pc + 1, 0xcb)),
                    Some(Access::Read(pc + 2, 0xff)),
                    Some(Access::Read(pc + 3, opcode)),
                    Some(Access::Read(0x28ff, 0x81)),
                ]
            );
            assert_eq!(
                trace.events[5],
                (!bit).then_some(Access::Write(0x28ff, value))
            );
            drop(trace);
            let whole = legacy.0.step_with_bus(&mut legacy.1).unwrap();
            assert_eq!(whole.cycles, if bit { 20 } else { 23 });
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
            assert_eq!((cpu.clone(), bus.clone()), legacy);
            assert_eq!((cpu, bus), (bounded.0, bounded.1));
        }
    }
    assert_eq!(encodings, 512);
}

pub fn indexed_cb_displacements_and_bit_address_flags() {
    for prefix in [0xdd, 0xfd] {
        for (base, displacement, address, xy) in [
            (0, 0xff, 0xffff, 0x28),
            (0xffff, 1, 0, 0),
            (0x2800, 0x80, 0x2780, 0x20),
            (0x27ff, 1, 0x2800, 0x28),
            (0x0800, 0, 0x0800, 8),
            (0, 0x7f, 0x7f, 0),
        ] {
            for (opcode, value, flags) in [(0x40, 0x28, 0x55), (0x7f, 0x80, 0x91)] {
                let (mut cpu, mut bus) = fixture(prefix, opcode, base, displacement, value);
                finish(&mut cpu, &mut bus, &mut BusState::default(), opcode, 0);
                assert_eq!(cpu.regs().f, flags | xy);
                assert_eq!(
                    bus.trace.borrow().events[4],
                    Some(Access::Read(address, value))
                );
                assert_eq!(bus.bytes[usize::from(address)], value);
            }
        }
        let mut cpu = Cpu::new_with_reset(ResetState::new(0xfffe, 0x8000));
        let mut bus = Bus::new();
        bus.bytes[0xfffe] = prefix;
        bus.bytes[0xffff] = 0xcb;
        bus.bytes[0] = 0x10;
        bus.bytes[1] = 0x06;
        bus.bytes[0x10] = 0x81;
        finish(&mut cpu, &mut bus, &mut BusState::default(), 0x06, 0);
        assert_eq!(cpu.regs().pc, 2);
        assert_eq!(cpu.regs().r, 2);
        assert_eq!(bus.bytes[0x10], 3);
    }
}

pub fn indexed_cb_live_fetches_and_latched_write() {
    for prefix in [0xdd, 0xfd] {
        for (initial, terminal, value, result, flags) in [
            (0x46, 0x04, 0x81, 0x03, 0x05),
            (0x04, 0x7d, 0x80, 0x80, 0xb9),
            (0x46, 0x15, 0x80, 0x01, 0x01),
            (0x46, 0x84, 0x81, 0x80, 0x01),
            (0x46, 0xc5, 0x80, 0x81, 0x01),
        ] {
            let (mut cpu, mut bus) = fixture(prefix, initial, 0x2900, 1, 0x55);
            let pc = usize::from(cpu.regs().pc);
            let mut state = BusState::default();
            phase(&mut cpu, &mut bus, &mut state, 4, false);
            phase(&mut cpu, &mut bus, &mut state, 4, false);
            bus.bytes[pc + 2] = 0xff;
            phase(&mut cpu, &mut bus, &mut state, 3, false);
            bus.bytes[pc + 2] = 0x80;
            bus.bytes[pc + 3] = terminal;
            phase(&mut cpu, &mut bus, &mut state, 5, false);
            assert_eq!(cpu.last_m1_fetch_count(), 2);
            bus.bytes[pc + 3] = 0;
            bus.bytes[0x28ff] = value;
            let bit = (0x40..0x80).contains(&terminal);
            let before = cpu.regs();
            phase(&mut cpu, &mut bus, &mut state, 4, bit);
            if !bit {
                assert_eq!(cpu.regs(), before);
                bus.bytes[0x28ff] = 0x55;
                phase(&mut cpu, &mut bus, &mut state, 3, true);
            }
            let mut expected = before;
            if !bit {
                copy_register(&mut expected, terminal & 7, result);
            }
            expected.f = flags;
            assert_eq!(cpu.regs(), expected);
            assert_eq!(bus.bytes[0x28ff], result);
            assert_eq!(bus.bytes[0x2901], 0x55);
        }
    }
}

pub fn indexed_cb_pending_phases_clone_suspend_and_reset() {
    for prefix in [0xdd, 0xfd] {
        for opcode in 0..=255u8 {
            for count in 1..schedule(opcode).len() {
                let (mut cpu, mut bus) = fixture(prefix, opcode, 0xffff, 1, 0x81);
                let mut state = BusState::default();
                for &clocks in &schedule(opcode)[..count] {
                    phase(&mut cpu, &mut bus, &mut state, clocks, false);
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
                reset_cpu.reset();
                reset_bus.clear_trace();
                reset_bus.bytes[0x100] = 0;
                phase(
                    &mut reset_cpu,
                    &mut reset_bus,
                    &mut BusState::default(),
                    4,
                    true,
                );
                assert_eq!(reset_bus.bytes[0], 0x81);
                assert_eq!(reset_cpu.regs().pc, 0x101);
            }
        }
    }
}

pub fn indexed_cb_prefixes_and_interrupt_boundaries() {
    for prefix in [0xdd, 0xfd] {
        for opcode in [0x04, 0x15, 0x46, 0x7f, 0x84, 0xc5] {
            for count in 1..schedule(opcode).len() {
                for nmi in [false, true] {
                    let (mut cpu, mut bus) = fixture(prefix, opcode, 0x2900, 0xff, 0x81);
                    let pc = usize::from(cpu.regs().pc);
                    let other = if prefix == 0xdd { 0xfd } else { 0xdd };
                    bus.bytes[pc..pc + 7]
                        .copy_from_slice(&[0xfb, other, prefix, 0xcb, 0xff, opcode, 0]);
                    cpu.step_with_bus(&mut bus).unwrap();
                    bus.clear_trace();
                    let mut legacy = (cpu.clone(), bus.clone());
                    let mut state = BusState::default();
                    phase(&mut cpu, &mut bus, &mut state, 4, false);
                    for &clocks in &schedule(opcode)[..count] {
                        phase(&mut cpu, &mut bus, &mut state, clocks, false);
                    }
                    let polls = bus.polls.get();
                    bus.nmi = nmi;
                    bus.irq = !nmi;
                    finish(&mut cpu, &mut bus, &mut state, opcode, count);
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

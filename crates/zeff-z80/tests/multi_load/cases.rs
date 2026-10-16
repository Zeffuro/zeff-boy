#[path = "support.rs"]
mod support;

use support::{Access, Bus, phase, seeded};
use zeff_z80::{BusState, Cpu, PrefixState, ResetState};

const OPCODES: [u8; 9] = [0x01, 0x11, 0x21, 0x31, 0x22, 0x2a, 0x32, 0x3a, 0x36];

pub fn all_load_encodings_match_legacy_and_literal_phases() {
    let mut encodings = 0;
    for prefix in [None, Some(0xdd), Some(0xfd)] {
        for opcode in OPCODES {
            if prefix.is_some() && opcode == 0x36 {
                continue;
            }
            encodings += 1;
            for value in [0, 1, 0x7f, 0x80, 0xff] {
                for address in [0x7000u16, 0xffff] {
                    let (mut cpu, mut bus) = seeded(value);
                    let start = cpu.regs().pc;
                    let mut cursor = start;
                    let mut expected_accesses = [None; 6];
                    let mut count = 0;
                    if let Some(prefix) = prefix {
                        bus.bytes[usize::from(cursor)] = prefix;
                        expected_accesses[count] = Some(Access::Read(cursor, prefix));
                        count += 1;
                        cursor += 1;
                    }
                    bus.bytes[usize::from(cursor)] = opcode;
                    expected_accesses[count] = Some(Access::Read(cursor, opcode));
                    count += 1;
                    cursor += 1;
                    let [low, high] = address.to_le_bytes();
                    let operand = if opcode == 0x36 { value } else { low };
                    bus.bytes[usize::from(cursor)] = operand;
                    expected_accesses[count] = Some(Access::Read(cursor, operand));
                    count += 1;
                    cursor += 1;
                    if opcode != 0x36 {
                        bus.bytes[usize::from(cursor)] = high;
                        expected_accesses[count] = Some(Access::Read(cursor, high));
                        count += 1;
                        cursor += 1;
                    }
                    let data_low = value ^ 0x3c;
                    bus.bytes[usize::from(address)] = data_low;
                    bus.bytes[usize::from(address.wrapping_add(1))] = value ^ 0xa5;
                    let selected = match prefix {
                        Some(0xdd) => cpu.regs().ix,
                        Some(0xfd) => cpu.regs().iy,
                        _ => cpu.regs().hl(),
                    };
                    let mut expected_regs = cpu.regs();
                    expected_regs.pc = cursor;
                    expected_regs.r += 1 + u8::from(prefix.is_some());
                    match opcode {
                        0x01 => {
                            expected_regs.b = high;
                            expected_regs.c = low;
                        }
                        0x11 => {
                            expected_regs.d = high;
                            expected_regs.e = low;
                        }
                        0x21 | 0x2a => {
                            let result = if opcode == 0x21 {
                                address
                            } else {
                                expected_accesses[count] = Some(Access::Read(address, data_low));
                                expected_accesses[count + 1] =
                                    Some(Access::Read(address.wrapping_add(1), value ^ 0xa5));
                                count += 2;
                                u16::from_le_bytes([data_low, value ^ 0xa5])
                            };
                            match prefix {
                                Some(0xdd) => expected_regs.ix = result,
                                Some(0xfd) => expected_regs.iy = result,
                                _ => {
                                    let [h, l] = result.to_be_bytes();
                                    expected_regs.h = h;
                                    expected_regs.l = l;
                                }
                            }
                        }
                        0x31 => expected_regs.sp = address,
                        0x22 => {
                            let [low, high] = selected.to_le_bytes();
                            expected_accesses[count] = Some(Access::Write(address, low));
                            expected_accesses[count + 1] =
                                Some(Access::Write(address.wrapping_add(1), high));
                            count += 2;
                        }
                        0x32 => {
                            expected_accesses[count] = Some(Access::Write(address, value));
                            count += 1;
                        }
                        0x3a => {
                            expected_accesses[count] = Some(Access::Read(address, data_low));
                            count += 1;
                            expected_regs.a = data_low;
                        }
                        0x36 => {
                            expected_accesses[count] = Some(Access::Write(cpu.regs().hl(), value));
                            count += 1;
                        }
                        _ => unreachable!(),
                    }
                    let mut legacy = cpu.clone();
                    let mut legacy_bus = bus.clone();
                    let mut bounded = cpu.clone();
                    let mut bounded_bus = bus.clone();
                    let expected = legacy.step_with_bus(&mut legacy_bus).unwrap();
                    let mut prefix_state = PrefixState::default();
                    let mut bounded_clocks = 0;
                    loop {
                        let step = bounded
                            .step_bounded(&mut bounded_bus, &mut prefix_state)
                            .unwrap();
                        bounded_clocks += step.instruction.cycles;
                        if step.complete {
                            break;
                        }
                    }
                    assert_eq!(bounded, legacy);
                    assert_eq!(bounded_bus, legacy_bus);
                    assert_eq!(bounded_clocks, expected.cycles);
                    let mut state = BusState::default();
                    let m1s = 1 + usize::from(prefix.is_some());
                    let transfers = match opcode {
                        0x22 | 0x2a => 4,
                        0x32 | 0x3a => 3,
                        _ => 2,
                    };
                    for i in 0..m1s + transfers {
                        phase(
                            &mut cpu,
                            &mut bus,
                            &mut state,
                            if i < m1s { 4 } else { 3 },
                            i + 1 == m1s + transfers,
                        );
                        assert_eq!(state.prefix_pending(), prefix.is_some() && state.pending());
                    }
                    assert_eq!(expected.cycles, m1s as u32 * 4 + transfers as u32 * 3);
                    assert_eq!(cpu.regs(), expected_regs, "{prefix:?} {opcode:02x}");
                    assert_eq!(cpu, legacy);
                    assert_eq!(bus, legacy_bus);
                    assert_eq!(
                        &bus.trace.borrow().events[..count],
                        &expected_accesses[..count]
                    );
                    assert_eq!(bus.trace.borrow().count, count);
                }
            }
        }
    }
    assert_eq!(encodings, 25);
}

pub fn operands_and_data_latch_at_individual_transfers() {
    for opcode in [0x21, 0x2a, 0x22, 0x32, 0x3a] {
        let (mut cpu, mut bus) = seeded(0x9a);
        let pc = usize::from(cpu.regs().pc);
        bus.bytes[pc..pc + 3].copy_from_slice(&[opcode, 0x34, 0x70]);
        bus.bytes[0x7134] = 0x56;
        bus.bytes[0x7135] = 0x78;
        let mut state = BusState::default();
        phase(&mut cpu, &mut bus, &mut state, 4, false);
        bus.bytes[pc] = 0x76;
        phase(&mut cpu, &mut bus, &mut state, 3, false);
        assert_eq!(cpu.regs().hl(), 0x3456);
        bus.bytes[pc + 1] = 0xff;
        bus.bytes[pc + 2] = 0x71;
        let old_regs = cpu.regs();
        phase(&mut cpu, &mut bus, &mut state, 3, opcode == 0x21);
        if opcode == 0x21 {
            assert_eq!(cpu.regs().hl(), 0x7134);
            continue;
        }
        bus.bytes[pc + 2] = 0xee;
        phase(
            &mut cpu,
            &mut bus,
            &mut state,
            3,
            matches!(opcode, 0x32 | 0x3a),
        );
        match opcode {
            0x2a => {
                assert_eq!(cpu.regs().hl(), old_regs.hl());
                bus.bytes[0x7134] = 0xff;
                bus.bytes[0x7135] = 0x9b;
                phase(&mut cpu, &mut bus, &mut state, 3, true);
                assert_eq!(cpu.regs().hl(), 0x9b56);
            }
            0x22 => {
                assert_eq!(bus.bytes[0x7134], 0x56);
                bus.bytes[0x7134] = 0xcc;
                phase(&mut cpu, &mut bus, &mut state, 3, true);
                assert_eq!((bus.bytes[0x7134], bus.bytes[0x7135]), (0xcc, 0x34));
            }
            0x32 => assert_eq!(bus.bytes[0x7134], 0x9a),
            0x3a => assert_eq!(cpu.regs().a, 0x56),
            _ => unreachable!(),
        }
    }
    let (mut cpu, mut bus) = seeded(0x81);
    let pc = usize::from(cpu.regs().pc);
    bus.bytes[pc..pc + 2].copy_from_slice(&[0x36, 0x65]);
    let mut state = BusState::default();
    phase(&mut cpu, &mut bus, &mut state, 4, false);
    phase(&mut cpu, &mut bus, &mut state, 3, false);
    bus.bytes[pc + 1] = 0xff;
    phase(&mut cpu, &mut bus, &mut state, 3, true);
    assert_eq!(bus.bytes[0x3456], 0x65);
}

pub fn completed_writes_survive_copy_discard_and_reset() {
    let (mut cpu, mut bus) = seeded(0x81);
    let pc = usize::from(cpu.regs().pc);
    bus.bytes[pc..pc + 3].copy_from_slice(&[0x22, 0xff, 0xff]);
    let mut state = BusState::default();
    for clocks in [4, 3, 3, 3] {
        phase(&mut cpu, &mut bus, &mut state, clocks, false);
    }
    assert_eq!((bus.bytes[0xffff], bus.bytes[0]), (0x56, 0));
    let (mut candidate, mut candidate_bus, mut candidate_state) = (cpu.clone(), bus.clone(), state);
    phase(
        &mut candidate,
        &mut candidate_bus,
        &mut candidate_state,
        3,
        true,
    );
    assert_eq!(
        (candidate_bus.bytes[0xffff], candidate_bus.bytes[0]),
        (0x56, 0x34)
    );
    assert_eq!((bus.bytes[0xffff], bus.bytes[0]), (0x56, 0));
    bus.bytes[0xffff] = 0xab;
    let (mut copied, mut copied_bus, mut copied_state) = (cpu.clone(), bus.clone(), state);
    phase(&mut cpu, &mut bus, &mut state, 3, true);
    phase(&mut copied, &mut copied_bus, &mut copied_state, 3, true);
    assert_eq!(
        (cpu, bus.clone(), state),
        (copied, copied_bus, copied_state)
    );
    assert_eq!((bus.bytes[0xffff], bus.bytes[0]), (0xab, 0x34));
    candidate.reset();
    candidate_state = BusState::default();
    assert!(!candidate_state.pending());
    assert_eq!(
        (candidate_bus.bytes[0xffff], candidate_bus.bytes[0]),
        (0x56, 0x34)
    );
    let (mut reset_cpu, mut reset_bus) = seeded(0);
    let pc = usize::from(reset_cpu.regs().pc);
    reset_bus.bytes[pc..pc + 3].copy_from_slice(&[0x22, 0, 0x70]);
    let mut reset_state = BusState::default();
    for clocks in [4, 3, 3, 3] {
        phase(
            &mut reset_cpu,
            &mut reset_bus,
            &mut reset_state,
            clocks,
            false,
        );
    }
    reset_cpu.reset();
    reset_state = BusState::default();
    assert!(!reset_state.pending());
    assert_eq!(
        (reset_bus.bytes[0x7000], reset_bus.bytes[0x7001]),
        (0x56, 0)
    );
}

pub fn interrupts_and_suspension_wait_for_load_completion() {
    for nmi in [false, true] {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new();
        bus.bytes[..5].copy_from_slice(&[0xfb, 0x2a, 0, 0x70, 0]);
        bus.bytes[0x7000..0x7002].copy_from_slice(&[0x34, 0x12]);
        let mut state = BusState::default();
        phase(&mut cpu, &mut bus, &mut state, 4, true);
        bus.irq = !nmi;
        phase(&mut cpu, &mut bus, &mut state, 4, false);
        bus.nmi = nmi;
        let polls = bus.polls.get();
        for i in 0..4 {
            cpu.suspend();
            let saved = (cpu.clone(), state, bus.clone());
            assert!(cpu.step_bus_segment(&mut bus, &mut state).is_none());
            assert_eq!((cpu.clone(), state, bus.clone()), saved);
            cpu.resume();
            let (mut copy, mut copy_bus, mut copy_state) = (cpu.clone(), bus.clone(), state);
            assert!(!cpu.interrupts_enabled());
            phase(&mut cpu, &mut bus, &mut state, 3, i == 3);
            phase(&mut copy, &mut copy_bus, &mut copy_state, 3, i == 3);
            assert_eq!((&cpu, &bus, state), (&copy, &copy_bus, copy_state));
            assert_eq!(bus.polls.get(), polls);
            assert_eq!(bus.acknowledgements, 0);
        }
        assert!(cpu.interrupts_enabled());
        assert_eq!(cpu.regs().hl(), 0x1234);
        for (index, clocks) in [if nmi { 5 } else { 7 }, 3, 3].into_iter().enumerate() {
            let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
            assert_eq!(step.instruction.cycles, clocks);
            assert_eq!(step.complete, index == 2);
        }
        assert!(cpu.last_step_was_interrupt());
        assert_eq!(cpu.regs().pc, if nmi { 0x66 } else { 0x38 });
    }
}

pub fn wrapping_and_repeated_prefixes_preserve_load_progress() {
    let mut bus = Bus::new();
    bus.bytes[0xfffe..].copy_from_slice(&[0xdd, 0xfd]);
    bus.bytes[..4].copy_from_slice(&[0xdd, 0x2a, 0xff, 0x6f]);
    bus.bytes[0x6fff] = 0x34;
    bus.bytes[0x7000] = 0x12;
    let mut cpu = Cpu::new_with_reset(ResetState::new(0xfffe, 0));
    let mut legacy = cpu.clone();
    let mut legacy_bus = bus.clone();
    let expected = legacy.step_with_bus(&mut legacy_bus).unwrap();
    let mut state = BusState::default();
    for i in 0..8 {
        phase(
            &mut cpu,
            &mut bus,
            &mut state,
            if i < 4 { 4 } else { 3 },
            i == 7,
        );
    }
    assert_eq!(expected.cycles, 28);
    assert_eq!(cpu.regs().ix, 0x1234);
    assert_eq!(cpu.regs().iy, 0);
    assert_eq!(cpu.regs().pc, 4);
    assert_eq!(cpu, legacy);
    assert_eq!(bus, legacy_bus);
}

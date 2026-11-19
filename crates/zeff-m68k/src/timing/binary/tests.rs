use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::{Cpu, SUPERVISOR, StepOutcome};

pub(super) fn seed(opcode: u16, status: u16, odd: bool, value: u32) -> (PrefetchCpu, Bus, u32) {
    let operation = decode(opcode).unwrap();
    let Binary {
        size,
        mode,
        register,
        ..
    } = operation;
    let mut timed = cpu(status);
    for i in 0..8 {
        timed.registers.d[i] = 0x7fff_fff0 + i as u32;
        timed
            .registers
            .set_address_register(i, 0x2000_3008 + i as u32 * 4 + u32::from(odd));
    }
    timed.registers.d[1] = 0xbeef_fffe;
    let base = timed.registers.address_register(register).unwrap();
    let increment = if size == Size::Byte && register == 7 {
        2
    } else {
        size.bytes()
    };
    let (extensions, address) = match mode {
        2 | 3 => (vec![], base),
        4 => (vec![], base.wrapping_sub(increment)),
        5 => (vec![0xfffe], base.wrapping_sub(2)),
        6 => (vec![0x1702], base),
        7 => match register {
            0 => (vec![0x8030 + u16::from(odd)], 0xffff_8030 + u32::from(odd)),
            1 => (
                vec![0xabcd, 0x3010 + u16::from(odd)],
                0xabcd_3010 + u32::from(odd),
            ),
            2 => (
                vec![0x0200 + u16::from(odd)],
                timed.registers.pc.wrapping_add(0x202 + u32::from(odd)),
            ),
            3 => (
                vec![0x1732 + u16::from(odd)],
                timed.registers.pc.wrapping_add(0x32 + u32::from(odd)),
            ),
            4 if size == Size::Long => (vec![(value >> 16) as u16, value as u16], 0),
            4 => (vec![value as u16], 0),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    let words: Vec<_> = extensions.into_iter().chain([0x7201, 0x7402]).collect();
    timed.pipeline.as_mut().unwrap().words = [opcode, words[0]];
    let mut bus = Bus::default();
    let pc = timed.registers.pc & BUS_ADDRESS_MASK;
    bus.words.insert(pc, opcode);
    for (i, word) in words.iter().enumerate() {
        bus.words.insert(pc + 2 + i as u32 * 2, *word);
    }
    if !(mode == 7 && register == 4) {
        let bus_address = address & BUS_ADDRESS_MASK & !1;
        if size == Size::Byte {
            bus.words.insert(
                bus_address,
                if address & 1 == 0 {
                    ((value as u16 & 0xff) << 8) | 0x5a
                } else {
                    0xa500 | (value as u16 & 0xff)
                },
            );
        } else if size == Size::Long {
            bus.words.insert(bus_address, (value >> 16) as u16);
            bus.words
                .insert(address.wrapping_add(2) & BUS_ADDRESS_MASK, value as u16);
        } else {
            bus.words.insert(bus_address, value as u16);
        }
    }
    (timed, bus, address)
}

fn opcodes() -> Vec<u16> {
    let mut opcodes = Vec::new();
    for family in [8, 9, 11, 12, 13] {
        for opmode in 0..8 {
            for mode in 2..8 {
                for register in 0..if mode == 7 { 5 } else { 8 } {
                    let destination = if register == 7 {
                        7
                    } else {
                        (mode + register) & 7
                    };
                    let op = family << 12 | destination << 9 | opmode << 6 | mode << 3 | register;
                    if decode(op).is_some() {
                        opcodes.push(op);
                    }
                }
            }
        }
    }
    for subtract in [0, 0x0100] {
        for quick in 0..8 {
            for size in 0..3 {
                for mode in 2..8 {
                    for register in 0..if mode == 7 { 2 } else { 8 } {
                        opcodes.push(
                            0x5000 | subtract | quick << 9 | size << 6 | mode << 3 | register,
                        );
                    }
                }
            }
        }
    }
    opcodes
}

#[test]
fn all_admitted_shapes_banks_lanes_waits_and_quick_values_match_architectural_work() {
    for opcode in opcodes() {
        for status in [0x1f, SUPERVISOR | 0x1f] {
            for odd in [false, true] {
                if odd && decode(opcode).unwrap().size != Size::Byte {
                    continue;
                }
                for wait in [0, 1, 3] {
                    let (mut timed, mut bus, _) = seed(opcode, status, odd, 0x8001_fff0);
                    let mut raw = crate::test_bus::TestBus::default();
                    for (&address, &word) in &bus.words {
                        raw.word(address, word);
                    }
                    let mut architectural = Cpu {
                        registers: timed.registers,
                        stopped: false,
                    };
                    let expected = architectural.step(&mut raw);
                    bus.wait = wait;
                    let observed = timed.step_instruction(&mut bus).unwrap();
                    assert!(
                        matches!(expected, StepOutcome::Executed { nominal_cycles, .. } if observed.clocks == u64::from(nominal_cycles) + bus.transfers as u64 * u64::from(wait)),
                        "op={opcode:04x}"
                    );
                    assert_eq!(timed.registers, architectural.registers, "op={opcode:04x}");
                    for (&address, &word) in &bus.words {
                        assert_eq!(
                            word,
                            u16::from_be_bytes([raw.peek(address), raw.peek(address + 1)]),
                            "op={opcode:04x}"
                        );
                    }
                    assert_eq!(timed.queued_words(), Some([0x7201, 0x7402]));
                    assert_eq!(timed.clock().0, bus.clock);
                    let operation = decode(opcode).unwrap();
                    for event in &bus.events {
                        if let Event::Read(a, _) | Event::Write(a, _, _) = event {
                            let program = a.kind == AccessKind::Prefetch
                                || operation.mode == 7 && matches!(operation.register, 2 | 3);
                            assert_eq!(
                                a.function_code,
                                if program {
                                    crate::ea::program_space(&timed.registers)
                                } else {
                                    crate::ea::data_space(&timed.registers)
                                }
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn schedules_have_exact_extension_reads_tail_idle_and_low_first_rmw_writes() {
    for opcode in opcodes() {
        let operation = decode(opcode).unwrap();
        let Binary {
            mode,
            register,
            size,
            destination,
            family,
            ..
        } = operation;
        let (mut timed, mut bus, address) = seed(opcode, SUPERVISOR | 0x1f, false, 0x8123_4567);
        timed.step_instruction(&mut bus).unwrap();
        let mut expected = match (mode, register) {
            (4, _) => "i",
            (6, _) | (7, 3) => "if",
            (5, _) | (7, 0 | 2) => "f",
            (7, 1) => "ff",
            (7, 4) if size == Size::Long => "ff",
            (7, 4) => "f",
            _ => "",
        }
        .to_owned();
        if !(mode == 7 && register == 4) {
            expected.push_str(if size == Size::Long { "rr" } else { "r" });
        }
        expected.push('f');
        if matches!(destination, Destination::Memory) {
            expected.push_str(if size == Size::Long { "ww" } else { "w" });
        } else if family == 11
            && (size == Size::Long || matches!(destination, Destination::Address(_)))
        {
            expected.push('i');
        } else if matches!(destination, Destination::Address(_))
            && size == Size::Word
            && !(mode == 7 && register == 4)
        {
            expected.push_str("ii");
        } else if matches!(destination, Destination::Address(_)) && size == Size::Word
            || size == Size::Long && mode == 7 && register == 4
        {
            expected.push('j');
        } else if size == Size::Long {
            expected.push('i');
        }
        let actual: String = bus
            .events
            .iter()
            .map(|event| match event {
                Event::Idle(_, 2) => 'i',
                Event::Idle(_, 4) => 'j',
                Event::Read(a, _) if a.kind == AccessKind::Prefetch => 'f',
                Event::Read(_, _) => 'r',
                Event::Write(_, _, _) => 'w',
                _ => panic!(),
            })
            .collect();
        assert_eq!(actual, expected, "op={opcode:04x}");
        let writes: Vec<_> = bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Write(a, _, _) => Some(a.logical_address),
                _ => None,
            })
            .collect();
        assert_eq!(
            writes,
            if matches!(destination, Destination::Memory) {
                if size == Size::Long {
                    vec![address.wrapping_add(2), address]
                } else {
                    vec![address]
                }
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn address_aliases_observe_updated_active_a7_sign_extend_words_and_preserve_ccr() {
    for family in [9, 11, 13] {
        for (opmode, size) in [(3, Size::Word), (7, Size::Long)] {
            for mode in [3, 4] {
                for status in [0x1f, SUPERVISOR | 0x1f] {
                    let opcode = family << 12 | 7 << 9 | opmode << 6 | mode << 3 | 7;
                    let (mut timed, mut bus, _) = seed(opcode, status, false, 0xffff_fffe);
                    let initial = timed.registers;
                    let increment = size.bytes();
                    let updated = if mode == 3 {
                        initial.stack_pointer().wrapping_add(increment)
                    } else {
                        initial.stack_pointer().wrapping_sub(increment)
                    };
                    timed.step_instruction(&mut bus).unwrap();
                    let expected = match family {
                        9 => updated.wrapping_add(2),
                        13 => updated.wrapping_sub(2),
                        _ => updated,
                    };
                    assert_eq!(timed.registers.stack_pointer(), expected);
                    assert_eq!(
                        if initial.supervisor() {
                            timed.registers.usp
                        } else {
                            timed.registers.ssp
                        },
                        if initial.supervisor() {
                            initial.usp
                        } else {
                            initial.ssp
                        }
                    );
                    if family != 11 {
                        assert_eq!(timed.registers.status_register(), status);
                    } else {
                        assert_eq!(
                            timed.registers.status_register() & crate::EXTEND,
                            status & crate::EXTEND
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn unsupported_special_aliases_and_illegal_eas_never_reach_callbacks() {
    for opcode in 0u16..=u16::MAX {
        let family = opcode >> 12;
        if !matches!(family, 5 | 8 | 9 | 11 | 12 | 13) {
            continue;
        }
        let mode = (opcode >> 3) & 7;
        let opmode = (opcode >> 6) & 7;
        let invalid = mode == 7 && opcode & 7 >= 5
            || mode == 7
                && opcode & 7 >= 2
                && (family == 5 && opmode & 3 < 3 || opmode >= 4 && opmode != 7)
            || mode == 1 && opmode == 0 && matches!(family, 8 | 9 | 11 | 12 | 13);
        if !invalid {
            continue;
        }
        let mut timed = cpu(SUPERVISOR);
        timed.pipeline.as_mut().unwrap().words[0] = opcode;
        let initial = timed.clone();
        let mut bus = Bus::default();
        assert!(
            matches!(
                timed.step_instruction(&mut bus),
                Err(TimingFault::UnsupportedInstruction { .. })
            ),
            "op={opcode:04x}"
        );
        assert_eq!(timed, initial);
        assert!(bus.events.is_empty());
    }
    for opcode in [0x80c8, 0xc0c8] {
        let mut timed = cpu(SUPERVISOR);
        timed.pipeline.as_mut().unwrap().words[0] = opcode;
        let mut bus = Bus::default();
        assert!(matches!(
            timed.step_instruction(&mut bus),
            Err(TimingFault::UnsupportedInstruction { .. })
        ));
        assert!(bus.events.is_empty());
    }
}

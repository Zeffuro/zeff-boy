use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::{Cpu, EXTEND, SUPERVISOR, StepOutcome, ZERO};

fn seed(
    family: u16,
    size: Size,
    operand: (u16, u16),
    status: u16,
    odd: bool,
    value: u32,
) -> (PrefetchCpu, Bus, u32) {
    let (mode, register) = operand;
    let size_bits = match size {
        Size::Byte => 0,
        Size::Word => 1,
        Size::Long => 2,
    };
    let opcode = family | (size_bits << 6) | (mode << 3) | register;
    let mut timed = cpu(status);
    timed.registers.d[1] = 0xbeef_fffe;
    for i in 0..8 {
        timed
            .registers
            .set_address_register(i, 0x2000_3008 + i as u32 * 4 + u32::from(odd));
    }
    let base = timed
        .registers
        .address_register(usize::from(register))
        .unwrap();
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
        7 if register == 0 => {
            let word = 0x8030 + u16::from(odd);
            (vec![word], Size::Word.sign_extend(u32::from(word)))
        }
        7 => (
            vec![0xabcd, 0x3010 + u16::from(odd)],
            0xabcd_3010 + u32::from(odd),
        ),
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
    (timed, bus, address)
}

#[test]
fn all_memory_modes_sizes_registers_banks_lanes_and_waits_match_architectural_work() {
    for family in [0x4000, 0x4200, 0x4400, 0x4600, 0x4a00] {
        for size in [Size::Byte, Size::Word, Size::Long] {
            for mode in 2..=7 {
                for register in 0..if mode == 7 { 2 } else { 8 } {
                    for status in [0x1f, SUPERVISOR | 0x1f] {
                        for odd in [false, true] {
                            if odd && size != Size::Byte {
                                continue;
                            }
                            for wait in [0, 1, 3] {
                                let (mut timed, mut bus, _) =
                                    seed(family, size, (mode, register), status, odd, 0x8001_fff0);
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
                                    matches!(expected, StepOutcome::Executed { nominal_cycles, .. } if observed.clocks == u64::from(nominal_cycles) + bus.transfers as u64 * u64::from(wait))
                                );
                                assert_eq!(
                                    timed.registers, architectural.registers,
                                    "op={:04x}",
                                    observed.opcode
                                );
                                for (&address, &word) in &bus.words {
                                    assert_eq!(
                                        word,
                                        u16::from_be_bytes([
                                            raw.peek(address),
                                            raw.peek(address + 1)
                                        ])
                                    );
                                }
                                assert_eq!(timed.queued_words(), Some([0x7201, 0x7402]));
                                assert_eq!(timed.clock().0, bus.clock);
                                for event in &bus.events {
                                    if let Event::Read(a, _) | Event::Write(a, _, _) = event {
                                        assert_eq!(
                                            a.function_code,
                                            if a.kind == AccessKind::Prefetch {
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
        }
    }
}

#[test]
fn operand_reads_prefetch_then_low_first_writes_have_exact_addresses_and_data() {
    for family in [0x4000, 0x4200, 0x4400, 0x4600, 0x4a00] {
        for size in [Size::Byte, Size::Word, Size::Long] {
            for mode in 2..=7 {
                for register in 0..if mode == 7 { 2 } else { 1 } {
                    let (mut timed, mut bus, address) =
                        seed(family, size, (mode, register), 0x201f, false, 0x8123_4567);
                    let old = timed.registers;
                    let mut result_registers = old;
                    let value = size.truncate(0x8123_4567);
                    let result = match family {
                        0x4000 => {
                            crate::extend::arithmetic(&mut result_registers, size, 0, value, true)
                        }
                        0x4400 => crate::alu::subtract(&mut result_registers, size, 0, value, true),
                        0x4200 => 0,
                        0x4600 => size.truncate(!value),
                        _ => value,
                    };
                    timed.step_instruction(&mut bus).unwrap();
                    let prefix = match mode {
                        4 => "i",
                        5 => "f",
                        6 => "if",
                        7 if register == 0 => "f",
                        7 => "ff",
                        _ => "",
                    };
                    let mut expected = prefix.to_owned();
                    expected.push_str(if size == Size::Long { "rrf" } else { "rf" });
                    if family != 0x4a00 {
                        expected.push_str(if size == Size::Long { "ww" } else { "w" });
                    }
                    let actual: String = bus
                        .events
                        .iter()
                        .map(|e| match e {
                            Event::Idle(_, 2) => 'i',
                            Event::Read(a, _) if a.kind == AccessKind::Prefetch => 'f',
                            Event::Read(_, _) => 'r',
                            Event::Write(_, _, _) => 'w',
                            _ => panic!(),
                        })
                        .collect();
                    assert_eq!(actual, expected);
                    let writes: Vec<_> = bus
                        .events
                        .iter()
                        .filter_map(|e| match e {
                            Event::Write(a, word, _) => Some((a.logical_address, *word)),
                            _ => None,
                        })
                        .collect();
                    if family == 0x4a00 {
                        assert!(writes.is_empty());
                    } else if size == Size::Long {
                        assert_eq!(
                            writes,
                            [
                                (address + 2, result as u16),
                                (address, (result >> 16) as u16)
                            ]
                        );
                    } else {
                        assert_eq!(
                            writes,
                            [(
                                address,
                                if size == Size::Byte && address & 1 == 0 {
                                    (result as u16) << 8
                                } else {
                                    result as u16
                                }
                            )]
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn negx_zero_is_sticky_and_clear_reads_even_when_operand_does_not_affect_result() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        for (status, value, expected) in [
            (ZERO, 0, ZERO),
            (0, 0, 0),
            (EXTEND | ZERO, size.mask(), EXTEND | crate::CARRY | ZERO),
            (ZERO, 1, EXTEND | crate::CARRY | crate::NEGATIVE),
        ] {
            let (mut timed, mut bus, _) = seed(0x4000, size, (2, 0), status, false, value);
            timed.step_instruction(&mut bus).unwrap();
            assert_eq!(timed.registers.status_register() & 0x1f, expected);
        }
        let (mut timed, mut bus, _) = seed(0x4200, size, (2, 0), 0x201f, false, size.mask());
        bus.fail = Some(0);
        let initial = timed.registers;
        assert!(matches!(
            timed.step(&mut bus),
            Err(TimingFault::Bus {
                direction: Direction::Read,
                ..
            })
        ));
        assert_eq!(timed.registers, initial);
        assert!(
            bus.events
                .iter()
                .all(|event| !matches!(event, Event::Write(..)))
        );
    }
}

#[test]
fn every_failure_preserves_address_and_alu_commits_completed_writes_and_freezes() {
    for family in [0x4000, 0x4200, 0x4400, 0x4600, 0x4a00] {
        for size in [Size::Byte, Size::Word, Size::Long] {
            for mode in 2..=7 {
                let register = if mode == 7 { 1 } else { 7 };
                for status in [0x1f, SUPERVISOR | 0x1f] {
                    let (mut completed, mut full, _) =
                        seed(family, size, (mode, register), status, false, 0x8123_4567);
                    full.wait = 1;
                    completed.step_instruction(&mut full).unwrap();
                    let last_read = full
                        .events
                        .iter()
                        .filter(|e| !matches!(e, Event::Idle(..)))
                        .position(|e| matches!(e, Event::Read(a, _) if a.kind == AccessKind::Data))
                        .unwrap()
                        + usize::from(size == Size::Long);
                    for fail in 0..full.transfers {
                        let (mut timed, mut bus, _) =
                            seed(family, size, (mode, register), status, false, 0x8123_4567);
                        let initial = timed.registers;
                        let initial_queue = timed.queued_words();
                        let mut expected_memory = bus.words.clone();
                        bus.fail = Some(fail);
                        bus.wait = 1;
                        assert!(matches!(timed.step(&mut bus), Err(TimingFault::Bus { .. })));
                        assert_eq!(timed.registers.pc, initial.pc);
                        assert_eq!(timed.queued_words(), initial_queue);
                        assert_eq!(
                            timed.registers.status_register(),
                            if fail > last_read {
                                if size == Size::Long
                                    && matches!(family, 0x4200 | 0x4600)
                                    && fail == last_read + 1
                                {
                                    let result = if family == 0x4200 { 0 } else { !0x4567u16 };
                                    let nz = if result == 0 { 4 } else { (result >> 12) & 8 };
                                    (initial.status_register() & !0xf) | nz
                                } else if size == Size::Long
                                    && matches!(family, 0x4000 | 0x4400)
                                    && fail == last_read + 1
                                {
                                    (initial.status_register() & !0x1f) | 0x19
                                } else {
                                    completed.registers.status_register()
                                }
                            } else {
                                initial.status_register()
                            }
                        );
                        assert_eq!(
                            timed.registers.stack_pointer(),
                            if mode == 4 || mode == 3 && (size != Size::Long || fail != 0) {
                                completed.registers.stack_pointer()
                            } else {
                                initial.stack_pointer()
                            }
                        );
                        let mut seen = 0;
                        for event in &full.events {
                            if matches!(event, Event::Idle(..)) {
                                continue;
                            }
                            if seen == fail {
                                break;
                            }
                            if let Event::Write(a, value, _) = event {
                                let old = expected_memory[&a.bus_address];
                                expected_memory.insert(
                                    a.bus_address,
                                    match a.lanes {
                                        ByteLanes::Both => *value,
                                        ByteLanes::Upper => (old & 0xff) | (value & 0xff00),
                                        ByteLanes::Lower => (old & 0xff00) | (value & 0xff),
                                    },
                                );
                            }
                            seen += 1;
                        }
                        assert_eq!(bus.words, expected_memory);
                        assert_eq!(timed.clock().0, bus.clock);
                        assert!(!timed.trace_pending());
                        let saved = bus.events.clone();
                        assert_eq!(timed.step_boundary(&mut bus), Err(TimingFault::Frozen));
                        assert_eq!(bus.events, saved);
                    }
                }
            }
        }
    }
}

#[test]
fn illegal_forms_are_unsupported_before_any_callback() {
    for family in [0x4000, 0x4200, 0x4400, 0x4600, 0x4a00] {
        for size in [0, 0x40, 0x80] {
            for operand in [8, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f] {
                let mut timed = cpu(0x2000);
                let opcode = family | size | operand;
                timed.pipeline.as_mut().unwrap().words[0] = opcode;
                let initial = timed.clone();
                let mut bus = Bus::default();
                assert!(matches!(
                    timed.step(&mut bus),
                    Err(TimingFault::UnsupportedInstruction { .. })
                ));
                assert_eq!(timed, initial);
                assert!(bus.events.is_empty());
            }
        }
    }
}

#[test]
fn queued_tail_precedes_writes_and_long_words_wrap_on_external_bus() {
    let (mut timed, mut bus, _) = seed(0x4640, Size::Word, (2, 0), 0x2000, false, 0x1234);
    let tail = timed.registers.pc.wrapping_add(4);
    timed.registers.a[0] = tail;
    bus.words.insert(tail & BUS_ADDRESS_MASK, 0x1234);
    timed.step_instruction(&mut bus).unwrap();
    assert_eq!(timed.queued_words(), Some([0x7201, 0x1234]));
    assert_eq!(bus.words[&(tail & BUS_ADDRESS_MASK)], 0xedcb);
    let (mut timed, mut bus, _) = seed(0x4600, Size::Long, (2, 0), 0, false, 0);
    timed.registers.a[0] = 0xffff_fffe;
    bus.words.insert(0xfffffe, 0x1234);
    bus.words.insert(0, 0x5678);
    timed.step_instruction(&mut bus).unwrap();
    let writes: Vec<_> = bus
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Write(a, value, _) => Some((a.logical_address, a.bus_address, *value)),
            _ => None,
        })
        .collect();
    assert_eq!(writes, [(0, 0, 0xa987), (0xffff_fffe, 0xfffffe, 0xedcb)]);
    let (mut timed, mut bus, _) = seed(0x4a00, Size::Word, (2, 0), 0, false, 0);
    timed.registers.a[0] = 0x0100_0001;
    assert_eq!(
        timed.step(&mut bus),
        Err(TimingFault::AddressError {
            address: 0x0100_0001
        })
    );
    assert!(bus.events.is_empty());
    assert_eq!(timed.step(&mut bus), Err(TimingFault::Frozen));
}

#[test]
fn pending_trace_follows_completed_memory_work_and_preserves_result_flags_in_frame() {
    for family in [0x4000, 0x4200, 0x4400, 0x4600, 0x4a00] {
        for status in [crate::TRACE | 0x1f, crate::TRACE | SUPERVISOR | 0x1f] {
            let (mut timed, mut bus, _) =
                seed(family, Size::Long, (2, 0), status, false, 0x8123_4567);
            bus.handler(9);
            timed.step_instruction(&mut bus).unwrap();
            assert!(timed.trace_pending());
            let flags = timed.registers.status_register();
            let sp = timed.registers.ssp;
            let boundary = bus.events.len();
            assert!(
                matches!(bus.events.last(), Some(Event::Read(a, _)) if family == 0x4a00 && a.kind == AccessKind::Prefetch)
                    || matches!(bus.events.last(), Some(Event::Write(a, _, _)) if family != 0x4a00 && a.kind == AccessKind::Data)
            );
            assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), Some(34));
            assert!(!timed.trace_pending());
            assert!(matches!(bus.events[boundary], Event::Idle(_, 4)));
            assert_eq!(bus.words[&(sp.wrapping_sub(6) & BUS_ADDRESS_MASK)], flags);
            let saved = bus.events.clone();
            assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), None);
            assert_eq!(bus.events, saved);
        }
    }
}
